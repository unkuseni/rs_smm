use std::collections::VecDeque;

use bybit::WsTrade;
use ndarray::{Array1, Array2};
use skeleton::util::helpers::StrategyConfig;
use skeleton::util::{
    ema::EMA,
    localorderbook::{calculate_weighted_ask, calculate_weighted_bid, LocalBook},
};

use super::{
    imbalance::{calculate_ofi, imbalance_ratio, trade_imbalance, voi},
    impact::{avg_trade_price, expected_return, mid_price_basis},
    linear_reg::mid_price_regression,
};

/// The engine tracks rolling market-microstructure features per symbol and
/// combines them into a single skew signal consumed by the market maker.
///
/// Feature weights, deadbands, gates and the prediction window are driven by
/// a StrategyConfig so the signal mix can be tuned from config.toml (and
/// A/B tested through the offline backtest harness) instead of editing code.
#[derive(Clone, Debug)]
pub struct Engine {
    pub imbalance_ratio: f64,
    pub deep_imbalance_ratio: Vec<f64>,
    pub voi: f64,
    pub deep_ofi: Vec<f64>,
    /// Mean deep OFI normalized by the depth-weighted volume at the widest
    /// requested depth: a scale-free order-flow imbalance in [-1, 1] that
    /// enters the skew with its magnitude (Cont, Kukanov & Stoikov 2011).
    pub ofi_scaled: f64,
    /// The EMA-smoothed imbalance quantized into 5 regimes (-2..=2),
    /// following Cartea, Donnelly & Jaimungal (2018), who note the raw
    /// imbalance is noisy and discretize it into finite states.
    pub imbalance_regime: i8,
    imb_ema: EMA,
    pub trade_imb: f64,
    pub mid_price_basis: f64,
    pub price_basis: PriceBasis,
    pub avg_trade_price: f64,
    pub predicted_price: f64,
    pub skew: f64,
    mid_prices: VecDeque<f64>,
    /// One row per tick: [voi, imbalance_ratio, mid_price_basis, ofi_scaled].
    features: VecDeque<[f64; 4]>,
    ticks_since_refit: usize,
    pub tick_window: usize,
    strategy: StrategyConfig,
}

impl Engine {
    /// Creates a new Engine with a lookback window of tick_window updates
    /// and the given strategy configuration.
    pub fn new(tick_window: usize, strategy: StrategyConfig) -> Self {
        Self {
            imbalance_ratio: 0.0,
            deep_imbalance_ratio: Vec::new(),
            voi: 0.0,
            deep_ofi: Vec::new(),
            ofi_scaled: 0.0,
            imbalance_regime: 0,
            imb_ema: EMA::new(tick_window, None),
            trade_imb: 0.0,
            mid_price_basis: 0.0,
            price_basis: PriceBasis::new(tick_window),
            avg_trade_price: 0.0,
            predicted_price: 0.0,
            skew: 0.0,
            mid_prices: VecDeque::new(),
            features: VecDeque::new(),
            ticks_since_refit: 0,
            tick_window,
            strategy,
        }
    }

    /// Updates all features with the latest order book and trade data.
    ///
    /// # Arguments
    ///
    /// * curr_book - The current order book.
    /// * prev_book - The previous order book.
    /// * curr_trades - The current trades.
    /// * prev_trades - The previous trades.
    /// * prev_avg - The average trade price of the previous update.
    /// * depth - The list of depths at which to compute features.
    pub fn update(
        &mut self,
        curr_book: &LocalBook,
        prev_book: &LocalBook,
        curr_trades: &VecDeque<WsTrade>,
        prev_trades: &VecDeque<WsTrade>,
        prev_avg: &f64,
        depth: Vec<usize>,
    ) {
        let Some(&first_depth) = depth.first() else {
            return;
        };

        // Update imbalance ratio (top of book).
        self.imbalance_ratio = imbalance_ratio(curr_book, Some(first_depth));

        // Smooth the (deadbanded) imbalance and quantize it into a regime.
        self.imb_ema.update(self.imbalance_ratio);
        self.imbalance_regime = quantize_regime(self.imb_ema.value());

        // Update deep imbalance ratios at every requested depth.
        self.deep_imbalance_ratio = depth
            .iter()
            .map(|v| imbalance_ratio(curr_book, Some(*v)))
            .collect();

        // Update volume of interest.
        self.voi = voi(curr_book, prev_book, Some(first_depth));

        // Update deep order flow imbalance at every requested depth.
        self.deep_ofi = depth
            .iter()
            .map(|v| calculate_ofi(curr_book, prev_book, Some(*v)))
            .collect();

        // Normalize the mean deep OFI by the depth-weighted volume at the
        // widest requested depth so the magnitude (not just the sign) can
        // enter the skew.
        let widest = depth.iter().copied().max().unwrap_or(first_depth);
        let wbid = calculate_weighted_bid(curr_book, widest);
        let wask = calculate_weighted_ask(curr_book, widest);
        let depth_volume = wbid + wask;
        let mean_ofi = self.deep_ofi.iter().sum::<f64>() / self.deep_ofi.len().max(1) as f64;
        self.ofi_scaled = if depth_volume > 0.0 {
            (mean_ofi / depth_volume).clamp(-1.0, 1.0)
        } else {
            0.0
        };

        // Update trade imbalance over the current trade window.
        self.trade_imb = trade_imbalance(curr_trades);

        // Update the average trade price: the incremental trade VWAP
        // normalized by the USD value of one tick (the book's tick size for
        // USDT-margined linear contracts).
        self.avg_trade_price = avg_trade_price(
            curr_book.get_mid_price(),
            Some(prev_trades),
            curr_trades,
            *prev_avg,
            curr_book.tick_size * curr_book.contract_size,
        );

        // Update the basis of the average trade price relative to the mid price.
        self.mid_price_basis = mid_price_basis(
            prev_book.get_mid_price(),
            curr_book.get_mid_price(),
            self.avg_trade_price,
        );

        // Update the EMA of the basis.
        self.price_basis.update(self.mid_price_basis);

        // Maintain a rolling window of mid prices for the regression.
        self.mid_prices.push_back(curr_book.get_mid_price());
        if self.mid_prices.len() > self.tick_window {
            self.mid_prices.pop_front();
        }

        // Maintain a rolling window of features for the regression.
        self.features.push_back([
            self.voi,
            self.imbalance_ratio,
            self.mid_price_basis,
            self.ofi_scaled,
        ]);
        if self.features.len() > self.tick_window {
            self.features.pop_front();
        }

        // Refit the linear model on a slower cadence to keep the hot path cheap.
        self.ticks_since_refit += 1;
        let refit_interval = (self.tick_window / 10).max(1);
        if self.features.len() >= self.tick_window && self.ticks_since_refit >= refit_interval {
            self.predicted_price = match self.predict_price(curr_book.get_spread_in_bps() as f64) {
                Ok(v) => v,
                Err(_) => {
                    let mid = curr_book.get_mid_price();
                    if self.strategy.ofi_impact_k > 0.0 && mid > 0.0 {
                        // Cont, Kukanov & Stoikov (2011): price changes are
                        // linear in OFI with a slope inversely proportional
                        // to depth; ofi_scaled is already depth-normalized.
                        mid * (1.0 + self.strategy.ofi_impact_k * self.ofi_scaled)
                    } else {
                        curr_book.get_microprice(Some(first_depth))
                    }
                }
            };
            self.ticks_since_refit = 0;
        }

        // Generate the composite skew signal.
        self.generate_skew(curr_book);
    }

    /// Fits a linear model on lagged feature pairs and returns the
    /// one-step-ahead prediction for the most recent feature row.
    ///
    /// Row t carries [f_t, f_{t-1}] (current features plus their one-lag
    /// values, 8 columns total) and predicts mid_{t+1}; Shen (2015) finds the
    /// instantaneous and lag-1 imbalance are the significant predictors, with
    /// mean reversion at further lags.
    fn predict_price(&mut self, curr_spread: f64) -> Result<f64, String> {
        if !curr_spread.is_finite() || curr_spread <= 0.0 {
            return Err("Invalid current spread".to_string());
        }
        if self.features.len() < 3 {
            return Err("Not enough history for regression".to_string());
        }

        let n = self.features.len();
        // Targets: mid prices from index 2 on, aligned with rows t=1..n-1
        // which predict mid_{t+1}.
        let mids: Vec<f64> = self.mid_prices.iter().skip(2).copied().collect();
        let rows: Vec<f64> = (1..n - 1)
            .flat_map(|t| {
                let f0 = self.features[t];
                let f1 = self.features[t - 1];
                [
                    f0[0], f0[1], f0[2], f0[3], f1[0], f1[1], f1[2], f1[3],
                ]
            })
            .collect();
        let x = Array2::from_shape_vec((n - 2, 8), rows)
            .map_err(|e| format!("Failed to reshape features: {}", e))?;
        let y = Array1::from(mids);
        // mid_price_regression trains on all but the last row and predicts
        // the last row, so the model maps a feature row to the next mid.
        mid_price_regression(y, x, curr_spread)
    }

    /// Replaces the strategy configuration (used by config hot-reload).
    pub fn set_strategy(&mut self, strategy: StrategyConfig) {
        self.strategy = strategy;
    }

    /// The most recent mid price observed, if any.
    pub fn last_mid(&self) -> Option<f64> {
        self.mid_prices.back().copied()
    }

    /// Standard deviation of mid-price log returns over the rolling window.
    ///
    /// Feeds the quote generator's volatility-adaptive spread: wider quotes
    /// when recent mid-price moves are large (Xiong, Yamada & Terano 2015
    /// find quoting with volatility information increases maker returns, and
    /// Bieganowski & Slepaczuk 2026 show fixed spreads lose to adverse
    /// selection in volatile regimes).
    pub fn mid_return_vol(&self) -> f64 {
        let n = self.mid_prices.len();
        if n < 2 {
            return 0.0;
        }
        let mids: Vec<f64> = self.mid_prices.iter().copied().collect();
        let rets: Vec<f64> = mids
            .windows(2)
            .filter_map(|w| {
                if w[0] > 0.0 && w[1] > 0.0 {
                    Some((w[1] / w[0]).ln())
                } else {
                    None
                }
            })
            .collect();
        if rets.is_empty() {
            return 0.0;
        }
        let mean = rets.iter().sum::<f64>() / rets.len() as f64;
        let var = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rets.len() as f64;
        var.sqrt()
    }

    /// Generates a composite skew value by combining weighted market indicators:
    /// - Imbalance ratio (top of book and deep)
    /// - Volume of Interest (VOI)
    /// - Trade imbalance
    /// - Deep order flow imbalance (OFI, depth-normalized magnitude)
    /// - Predicted price movement
    ///
    /// Positive skew indicates buy pressure; negative skew indicates sell pressure.
    fn generate_skew(&mut self, book: &LocalBook) {
        let s = &self.strategy;

        // Weighted top-of-book imbalance.
        let imb = self.imbalance_ratio * s.imb_weight;

        // Weighted average deep imbalance (only significant values).
        let deep_imb = {
            let value = self.deep_imbalance_ratio.iter().sum::<f64>()
                / self.deep_imbalance_ratio.len().max(1) as f64;
            if value.abs() > s.imb_deadband {
                value * s.deep_imb_weight
            } else {
                0.0
            }
        };

        // VOI contributes only its sign, scaled by its weight.
        let voi = match self.voi {
            v if v > 0.0 => s.voi_weight,
            v if v < 0.0 => -s.voi_weight,
            _ => 0.0,
        };

        // Trade imbalance only contributes beyond its deadband.
        let trade_imb = {
            let v = self.trade_imb;
            if v.abs() > s.trade_deadband {
                v * s.trade_weight
            } else {
                0.0
            }
        };

        // Deep OFI contributes its depth-normalized magnitude.
        let deep_ofi = self.ofi_scaled * s.deep_ofi_weight;

        // Predicted price movement relative to the current mid, gated by the
        // trade-price basis to avoid acting on noise.
        let mid = book.get_mid_price();
        let basis_frac = if mid > 0.0 {
            self.price_basis.current_basis() / mid
        } else {
            0.0
        };
        let exp_ret = expected_return(mid, self.predicted_price);
        let predicted_value = if exp_ret > s.predict_gate && basis_frac > s.basis_gate {
            s.predict_weight
        } else if exp_ret >= s.predict_gate * 0.6 || basis_frac >= s.basis_gate * 0.6 {
            0.5 * s.predict_weight
        } else if exp_ret < -s.predict_gate && basis_frac < -s.basis_gate {
            -s.predict_weight
        } else if exp_ret <= -s.predict_gate * 0.6 || basis_frac <= -s.basis_gate * 0.6 {
            -0.5 * s.predict_weight
        } else {
            0.0
        };

        // Regime-smoothed imbalance offset (Cartea et al. 2018).
        let regime = s.regime_weight * self.imbalance_regime as f64;

        self.skew =
            (imb + deep_imb + voi + trade_imb + deep_ofi + predicted_value + regime).clamp(-1.0, 1.0);
    }
}

/// Quantizes a smoothed imbalance into one of 5 regimes:
/// -2 (strong sell), -1 (sell), 0 (neutral), 1 (buy), 2 (strong buy).
fn quantize_regime(v: f64) -> i8 {
    if v > 0.6 {
        2
    } else if v > 0.2 {
        1
    } else if v < -0.6 {
        -2
    } else if v < -0.2 {
        -1
    } else {
        0
    }
}

/// Tracks the EMA of the difference between the average trade price and the
/// mid price. The basis reverts toward zero, which makes it a useful
/// short-horizon predictor of mid-price direction.
#[derive(Debug, Clone)]
pub struct PriceBasis {
    basis_ema: EMA,
}

impl PriceBasis {
    /// Creates a new PriceBasis tracker with the given EMA window.
    pub fn new(window: usize) -> Self {
        Self {
            basis_ema: EMA::new(window, None),
        }
    }

    /// Updates the EMA with a new basis value and returns the current value.
    pub fn update(&mut self, basis: f64) -> f64 {
        self.basis_ema.update(basis);
        self.basis_ema.value()
    }

    /// Returns the current basis value.
    pub fn current_basis(&self) -> f64 {
        self.basis_ema.value()
    }

    /// Returns the historical basis values.
    pub fn basis_history(&self) -> Vec<f64> {
        self.basis_ema.arr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bybit::{Ask, Bid, TickDirection};

    fn make_book(bids: &[(f64, f64)], asks: &[(f64, f64)], ts: u64) -> LocalBook {
        let mut book = LocalBook::new();
        book.tick_size = 0.01;
        book.lot_size = 0.001;
        book.min_order_size = 0.001;
        book.min_notional = 1.0;
        book.post_only_max = 1000.0;
        book.update(
            bids.iter().map(|&(p, q)| Bid { price: p, qty: q }).collect(),
            asks.iter().map(|&(p, q)| Ask { price: p, qty: q }).collect(),
            ts,
        );
        book
    }

    fn trade(side: &str, volume: f64, price: f64) -> WsTrade {
        WsTrade::new(1, "BTCUSDT", side, volume, price, TickDirection::PlusTick, "t", false)
    }

    fn empty_trades() -> VecDeque<WsTrade> {
        VecDeque::new()
    }

    fn config(weights: [f64; 6]) -> StrategyConfig {
        StrategyConfig {
            imb_weight: weights[0],
            deep_imb_weight: weights[1],
            voi_weight: weights[2],
            trade_weight: weights[3],
            deep_ofi_weight: weights[4],
            predict_weight: weights[5],
            imb_deadband: 0.0,
            trade_deadband: 0.0,
            predict_gate: 0.0005,
            basis_gate: 0.0005,
            ..Default::default()
        }
    }

    #[test]
    fn trade_only_config_drives_skew() {
        let mut engine = Engine::new(100, config([0.0, 0.0, 0.0, 1.0, 0.0, 0.0]));
        let b1 = make_book(&[(100.0, 10.0)], &[(100.5, 10.0)], 1);
        let b2 = make_book(&[(100.0, 10.0)], &[(100.5, 10.0)], 2);
        let empty = empty_trades();
        let mut buys: VecDeque<WsTrade> = VecDeque::new();
        buys.push_back(trade("Buy", 1.0, 100.25));

        let prev_avg = 0.0;
        engine.update(&b2, &b1, &buys, &empty, &prev_avg, vec![3, 8]);

        // Only the trade imbalance weight is non-zero and all trades are buys:
        // skew must equal the full weight.
        assert!((engine.skew - 1.0).abs() < 1e-9, "skew was {}", engine.skew);
        assert!((engine.trade_imb - 1.0).abs() < 1e-9);

        let mut sells: VecDeque<WsTrade> = VecDeque::new();
        sells.push_back(trade("Sell", 1.0, 100.25));
        engine.update(&b1, &b2, &sells, &buys, &prev_avg, vec![3, 8]);
        assert!((engine.skew + 1.0).abs() < 1e-9, "skew was {}", engine.skew);
    }

    #[test]
    fn skew_stays_in_bounds_with_defaults() {
        let mut engine = Engine::new(100, StrategyConfig::default());
        let b1 = make_book(&[(100.0, 10.0)], &[(100.5, 10.0)], 1);
        let mut prev = b1.clone();
        let empty = empty_trades();
        let mut trades: VecDeque<WsTrade> = VecDeque::new();
        trades.push_back(trade("Buy", 1.0, 100.25));

        for i in 0..20 {
            let price = 100.0 + (i as f64) * 0.01;
            let next = make_book(&[(price, 10.0)], &[(price + 0.5, 10.0)], 2 + i);
            engine.update(&next, &prev, &trades, &empty, &0.0, vec![3, 8, 34]);
            assert!(engine.skew.abs() <= 1.0, "skew out of bounds: {}", engine.skew);
            assert!(engine.ofi_scaled.abs() <= 1.0);
            prev = next;
        }
    }

    #[test]
    fn ofi_scaled_is_normalized() {
        let mut engine = Engine::new(100, StrategyConfig::default());
        let b1 = make_book(&[(100.0, 10.0)], &[(100.5, 10.0)], 1);
        // A big bid-side improvement should produce positive OFI.
        let b2 = make_book(&[(100.4, 12.0)], &[(100.5, 10.0)], 2);
        let empty = empty_trades();
        engine.update(&b2, &b1, &empty, &empty, &0.0, vec![3, 8]);
        assert!(engine.ofi_scaled > 0.0, "ofi_scaled was {}", engine.ofi_scaled);
        assert!(engine.ofi_scaled <= 1.0);
    }

    #[test]
    fn regime_quantizes_smoothed_imbalance() {
        let mut engine = Engine::new(100, StrategyConfig::default());
        let empty = empty_trades();

        // Heavy bid side for many updates: the EMA-smoothed imbalance
        // converges above 0.6 -> regime +2.
        let mut prev = make_book(&[(100.0, 100.0)], &[(100.5, 10.0)], 1);
        for i in 0..80 {
            let next = make_book(&[(100.0, 100.0)], &[(100.5, 10.0)], 2 + i);
            engine.update(&next, &prev, &empty, &empty, &0.0, vec![3]);
            prev = next;
        }
        assert_eq!(engine.imbalance_regime, 2);

        // Heavy ask side: regime -2.
        let mut engine = Engine::new(100, StrategyConfig::default());
        let mut prev = make_book(&[(100.0, 10.0)], &[(100.5, 100.0)], 1);
        for i in 0..80 {
            let next = make_book(&[(100.0, 10.0)], &[(100.5, 100.0)], 2 + i);
            engine.update(&next, &prev, &empty, &empty, &0.0, vec![3]);
            prev = next;
        }
        assert_eq!(engine.imbalance_regime, -2);
    }

    #[test]
    fn ofi_impact_fallback_predicts_when_regression_cannot() {
        // tick_window = 2 so the refit fires after two updates; the
        // regression then fails (needs >= 3 rows) and the OFI impact
        // estimate takes over.
        let strategy = StrategyConfig {
            ofi_impact_k: 0.0005,
            ..Default::default()
        };
        let mut engine = Engine::new(2, strategy);
        let b1 = make_book(&[(100.0, 10.0)], &[(100.5, 10.0)], 1);
        // Consecutive bid-side improvements keep OFI positive through the
        // second update (the refit fires there).
        let b2 = make_book(&[(100.4, 12.0)], &[(100.5, 10.0)], 2);
        let empty = empty_trades();
        engine.update(&b2, &b1, &empty, &empty, &0.0, vec![3, 8]);
        let b3 = make_book(&[(100.8, 14.0)], &[(100.5, 10.0)], 3);
        engine.update(&b3, &b2, &empty, &empty, &0.0, vec![3, 8]);

        assert!(engine.ofi_scaled > 0.0);
        assert!(
            engine.predicted_price > b3.get_mid_price(),
            "OFI fallback should predict above the mid with positive OFI"
        );
    }

    #[test]
    fn volatility_measures_mid_moves() {
        let mut engine = Engine::new(100, StrategyConfig::default());
        let mut prev = make_book(&[(100.0, 10.0)], &[(100.5, 10.0)], 1);
        let empty = empty_trades();
        for i in 0..10 {
            let price = 100.0 + (i as f64) * 0.1;
            let next = make_book(&[(price, 10.0)], &[(price + 0.5, 10.0)], 2 + i);
            engine.update(&next, &prev, &empty, &empty, &0.0, vec![3]);
            prev = next;
        }
        assert!(engine.mid_return_vol() > 0.0);
    }
}
