use std::collections::VecDeque;

use bybit::WsTrade;
use ndarray::{Array1, Array2};
use skeleton::util::{ema::EMA, localorderbook::LocalBook};

use super::{
    imbalance::{calculate_ofi, imbalance_ratio, trade_imbalance, voi},
    impact::{avg_trade_price, expected_return, mid_price_basis},
    linear_reg::mid_price_regression,
};

/// Weight of the top-of-book imbalance ratio in the skew calculation.
const IMB_WEIGHT: f64 = 0.25;

/// Weight of the average deep-book imbalance ratio in the skew calculation.
const DEEP_IMB_WEIGHT: f64 = 0.10;

/// Weight of the volume of interest (VOI) in the skew calculation.
const VOI_WEIGHT: f64 = 0.10;

/// Weight of the trade imbalance in the skew calculation.
const TRADE_WEIGHT: f64 = 0.30;

/// Weight of the deep order flow imbalance (OFI) in the skew calculation.
const DEEP_OFI_WEIGHT: f64 = 0.10;

/// Weight of the predicted price movement in the skew calculation.
const PREDICT_WEIGHT: f64 = 0.15;

/// The engine tracks rolling market-microstructure features per symbol and
/// combines them into a single skew signal consumed by the market maker.
#[derive(Clone, Debug)]
pub struct Engine {
    pub imbalance_ratio: f64,
    pub deep_imbalance_ratio: Vec<f64>,
    pub voi: f64,
    pub deep_ofi: Vec<f64>,
    pub trade_imb: f64,
    pub mid_price_basis: f64,
    pub price_basis: PriceBasis,
    pub avg_trade_price: f64,
    pub predicted_price: f64,
    pub skew: f64,
    mid_prices: VecDeque<f64>,
    features: VecDeque<[f64; 3]>,
    ticks_since_refit: usize,
    pub tick_window: usize,
}

impl Engine {
    /// Creates a new `Engine` with a lookback window of `tick_window` updates.
    pub fn new(tick_window: usize) -> Self {
        Self {
            imbalance_ratio: 0.0,
            deep_imbalance_ratio: Vec::new(),
            voi: 0.0,
            deep_ofi: Vec::new(),
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
        }
    }

    /// Updates all features with the latest order book and trade data.
    ///
    /// # Arguments
    ///
    /// * `curr_book` - The current order book.
    /// * `prev_book` - The previous order book.
    /// * `curr_trades` - The current trades.
    /// * `prev_trades` - The previous trades.
    /// * `prev_avg` - The average trade price of the previous update.
    /// * `depth` - The list of depths at which to compute features.
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
            curr_book.tick_size,
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
        self.features
            .push_back([self.voi, self.imbalance_ratio, self.mid_price_basis]);
        if self.features.len() > self.tick_window {
            self.features.pop_front();
        }

        // Refit the linear model on a slower cadence to keep the hot path cheap.
        self.ticks_since_refit += 1;
        let refit_interval = (self.tick_window / 10).max(1);
        if self.features.len() >= self.tick_window && self.ticks_since_refit >= refit_interval {
            self.predicted_price = match self.predict_price(curr_book.get_spread_in_bps() as f64) {
                Ok(v) => v,
                Err(_) => curr_book.get_microprice(Some(first_depth)),
            };
            self.ticks_since_refit = 0;
        }

        // Generate the composite skew signal.
        self.generate_skew(curr_book);
    }

    /// Fits a linear model on all but the last observation and returns the
    /// one-step-ahead prediction for the most recent feature row.
    fn predict_price(&mut self, curr_spread: f64) -> Result<f64, String> {
        if !curr_spread.is_finite() || curr_spread <= 0.0 {
            return Err("Invalid current spread".to_string());
        }
        if self.features.len() < 2 {
            return Err("Not enough history for regression".to_string());
        }

        let mids: Vec<f64> = self.mid_prices.iter().copied().collect();
        let feats: Vec<f64> = self
            .features
            .iter()
            .flat_map(|v| v.iter().copied())
            .collect();
        let y = Array1::from(mids);
        let x = Array2::from_shape_vec((self.features.len(), 3), feats)
            .map_err(|e| format!("Failed to reshape features: {}", e))?;

        mid_price_regression(y, x, curr_spread)
    }

    /// Generates a composite skew value by combining weighted market indicators:
    /// - Imbalance ratio (top of book and deep)
    /// - Volume of Interest (VOI)
    /// - Trade imbalance
    /// - Deep order flow imbalance (OFI)
    /// - Predicted price movement
    ///
    /// Positive skew indicates buy pressure; negative skew indicates sell pressure.
    fn generate_skew(&mut self, book: &LocalBook) {
        // Weighted top-of-book imbalance.
        let imb = self.imbalance_ratio * IMB_WEIGHT;

        // Weighted average deep imbalance (only significant values).
        let deep_imb = {
            let value = self.deep_imbalance_ratio.iter().sum::<f64>()
                / self.deep_imbalance_ratio.len().max(1) as f64;
            match value {
                v if v > 0.20 => v * DEEP_IMB_WEIGHT,
                v if v < -0.20 => v * DEEP_IMB_WEIGHT,
                _ => 0.0,
            }
        };

        // VOI contributes only its sign, scaled by its weight.
        let voi = match self.voi {
            v if v > 0.0 => VOI_WEIGHT,
            v if v < 0.0 => -VOI_WEIGHT,
            _ => 0.0,
        };

        // Trade imbalance only contributes beyond a ±0.20 deadband.
        let trade_imb = match self.trade_imb {
            v if v > 0.20 => v * TRADE_WEIGHT,
            v if v < -0.20 => v * TRADE_WEIGHT,
            _ => 0.0,
        };

        // Deep OFI contributes only its sign, scaled by its weight.
        let deep_ofi = {
            let value = self.deep_ofi.iter().sum::<f64>() / self.deep_ofi.len().max(1) as f64;
            match value {
                v if v > 0.0 => DEEP_OFI_WEIGHT,
                v if v < 0.0 => -DEEP_OFI_WEIGHT,
                _ => 0.0,
            }
        };

        // Predicted price movement relative to the current mid, gated by the
        // trade-price basis to avoid acting on noise.
        let predicted_value = match self.predicted_price {
            v if expected_return(book.get_mid_price(), v) > 0.0005
                && (self.price_basis.current_basis() / book.get_mid_price()) > 0.0005 =>
            {
                PREDICT_WEIGHT
            }
            v if expected_return(book.get_mid_price(), v) >= 0.0005
                || (self.price_basis.current_basis() / book.get_mid_price()) >= 0.0003 =>
            {
                0.5 * PREDICT_WEIGHT
            }
            v if expected_return(book.get_mid_price(), v) < -0.0005
                && (self.price_basis.current_basis() / book.get_mid_price()) < -0.0005 =>
            {
                -PREDICT_WEIGHT
            }
            v if expected_return(book.get_mid_price(), v) <= -0.0005
                || (self.price_basis.current_basis() / book.get_mid_price()) <= -0.0003 =>
            {
                -0.5 * PREDICT_WEIGHT
            }
            _ => 0.0,
        };

        self.skew = imb + deep_imb + voi + trade_imb + deep_ofi + predicted_value;
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
    /// Creates a new `PriceBasis` tracker with the given EMA window.
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
