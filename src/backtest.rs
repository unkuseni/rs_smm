//! Offline replay/backtest harness.
//!
//! Replays a recording produced by the live loaders (config: record = "file")
//! through the real feature engine and a simulated quote generator:
//!
//! - order-book deltas are applied through the same LocalBook code paths the
//!   live loaders use (including the bba flag),
//! - resting maker orders fill conservatively on trade-through (a buy fills
//!   when a trade prints at or below its price, a sell at or above), which
//!   understates fills and never fabricates adverse ones,
//! - market-order rebalances fill at the current best bid/ask,
//! - maker/taker fees are charged at 2/5 bps.
//!
//! Output includes PnL, fill allocation per grid level, and the mean/std
//! ratio of per-update PnL deltas, so strategy-parameter changes can be A/B
//! tested on recorded data instead of live funds.

use std::collections::{HashMap, VecDeque};

use bybit::{Ask, Bid, FastExecData, WsTrade};
use skeleton::exchanges::exchange::{MarketEvent, PrivateData};
use skeleton::exchanges::ex_bybit::BybitPrivate;
use skeleton::util::helpers::{Config, StrategyConfig};
use skeleton::util::localorderbook::LocalBook;
use skeleton::util::recorder::ReplayStream;

use crate::features::engine::Engine;
use crate::trader::quote_gen::QuoteGenerator;

/// Assumed maker fee per fill (2 bps) for the simulated account.
const MAKER_FEE: f64 = 0.0002;
/// Assumed taker fee per fill (5 bps) for the simulated account.
const TAKER_FEE: f64 = 0.0005;
/// Capacity of the rolling trade buffer, mirroring the live loaders.
const TRADE_BUFFER_CAP: usize = 5000;

/// Per-symbol statistics produced by a replay run.
#[derive(Debug, Clone, Default)]
pub struct SymbolReport {
    pub symbol: String,
    pub book_updates: u64,
    pub trades: u64,
    pub grid_refreshes: u64,
    pub maker_fills: u64,
    pub maker_notional: f64,
    pub maker_fees: f64,
    pub taker_fills: u64,
    pub taker_notional: f64,
    pub taker_fees: f64,
    /// Funding cost accrued on held inventory (positive = paid).
    pub funding_paid: f64,
    pub final_position: f64,
    pub final_mid: f64,
    pub total_pnl: f64,
    /// Mean/std of per-book-update PnL deltas over the run.
    pub sharpe: f64,
    /// Maker fills per grid level (0 = closest to the anchor price).
    pub fill_by_level: Vec<u64>,
    /// Running realized/quoted spread ratio (adverse selection, < 1 = picked
    /// off), in-place grid amendments, and market-order rebalances.
    pub adverse_ratio: f64,
    pub grid_amends: u64,
    pub rebalance_count: u64,
    /// Horizon-prediction evaluation: how often the predicted direction
    /// matched the realized horizon outcome (only when a horizon is set).
    pub pred_evals: u64,
    pub pred_hits: u64,
}

/// The result of one replay run.
#[derive(Debug)]
pub struct BacktestReport {
    pub duration_ms: u64,
    pub symbols: Vec<SymbolReport>,
}

struct SymState {
    book: LocalBook,
    trades: VecDeque<WsTrade>,
    prev_book: Option<LocalBook>,
    prev_trades: Option<VecDeque<WsTrade>>,
    engine: Engine,
    gen: QuoteGenerator,
    warmup: usize,
    pending_execs: VecDeque<FastExecData>,
    cash: f64,
    pnl_path: Vec<f64>,
    last_fund_ts: u64,
    /// (timestamp, mid, predicted) at each book update after warmup, for the
    /// offline horizon-prediction evaluation.
    pred_log: Vec<(u64, f64, f64)>,
    report: SymbolReport,
}

/// Funding cost for holding `position` at `mark` for `dt_hours` at
/// `bps_per_hour`. Long positions pay funding when the rate is positive.
pub fn funding_pnl(position: f64, mark: f64, bps_per_hour: f64, dt_hours: f64) -> f64 {
    -position * mark * (bps_per_hour / 10_000.0) * dt_hours
}

/// Offline evaluation of the engine's horizon predictions against the
/// recorded future. For each prediction made at (ts, mid), the outcome is
/// determined by the same horizon rule the engine trains on (milliseconds,
/// or +-bps barrier first touch) applied to the recorded mids that follow.
/// Returns (evaluated, hits). Only meaningful when a horizon is configured;
/// returns (0, 0) otherwise.
pub fn evaluate_predictions(
    entries: &[(u64, f64, f64)],
    strategy: &StrategyConfig,
) -> (u64, u64) {
    let horizon_ms = strategy.predict_horizon_ms;
    let bps = strategy.predict_horizon_bps;
    if horizon_ms == 0 && bps <= 0.0 {
        return (0, 0);
    }
    let mut evaluated = 0u64;
    let mut hits = 0u64;
    for (i, (ts, mid, pred)) in entries.iter().enumerate() {
        let dir = if *pred > *mid {
            1i8
        } else if *pred < *mid {
            -1
        } else {
            0
        };
        if dir == 0 {
            continue;
        }
        let outcome = if bps > 0.0 {
            let up = mid * (1.0 + bps / 10_000.0);
            let down = mid * (1.0 - bps / 10_000.0);
            let mut outcome = 0i8;
            for (ts2, mid2, _) in &entries[i + 1..] {
                if horizon_ms > 0 && ts2.saturating_sub(*ts) > horizon_ms {
                    break;
                }
                if *mid2 >= up {
                    outcome = 1;
                    break;
                }
                if *mid2 <= down {
                    outcome = -1;
                    break;
                }
            }
            outcome
        } else {
            let mut outcome = 0i8;
            for (ts2, mid2, _) in &entries[i + 1..] {
                if ts2.saturating_sub(*ts) >= horizon_ms {
                    outcome = if *mid2 > *mid {
                        1
                    } else if *mid2 < *mid {
                        -1
                    } else {
                        0
                    };
                    break;
                }
            }
            outcome
        };
        if outcome != 0 {
            evaluated += 1;
            if outcome == dir {
                hits += 1;
            }
        }
    }
    (evaluated, hits)
}

/// Applies one sweep parameter to a strategy config by key name. Used by the
/// `--sweep` mode of the backtest binary to run a parameter sensitivity
/// sweep over a recorded session.
pub fn sweep_value(key: &str, value: f64, strategy: &mut StrategyConfig) -> Result<(), String> {
    match key {
        "imb_weight" => strategy.imb_weight = value,
        "deep_imb_weight" => strategy.deep_imb_weight = value,
        "trade_weight" => strategy.trade_weight = value,
        "voi_weight" => strategy.voi_weight = value,
        "deep_ofi_weight" => strategy.deep_ofi_weight = value,
        "predict_weight" => strategy.predict_weight = value,
        "vol_spread_scaling" => strategy.vol_spread_scaling = value,
        "inventory_adjustment" => strategy.inventory_adjustment = value,
        "aggression_max" => strategy.aggression_max = value,
        "rebalance_threshold" => strategy.rebalance_threshold = value,
        "ofi_impact_k" => strategy.ofi_impact_k = value,
        "predict_horizon_ms" => strategy.predict_horizon_ms = value.max(0.0) as u64,
        "predict_horizon_bps" => strategy.predict_horizon_bps = value,
        "predict_gate" => strategy.predict_gate = value,
        "basis_gate" => strategy.basis_gate = value,
        "regime_weight" => strategy.regime_weight = value,
        "as_gamma" => strategy.as_gamma = value,
        "funding_bps_per_hour" => strategy.funding_bps_per_hour = value,
        other => {
            return Err(format!(
                "unknown sweep key: {} (supported: imb_weight, deep_imb_weight, trade_weight, \
                 voi_weight, deep_ofi_weight, predict_weight, vol_spread_scaling, \
                 inventory_adjustment, aggression_max, rebalance_threshold, ofi_impact_k, \
                 predict_horizon_ms, predict_horizon_bps, predict_gate, basis_gate, \
                 regime_weight, as_gamma, funding_bps_per_hour)",
                other
            ))
        }
    }
    Ok(())
}

impl SymState {
    /// Books a simulated fill: updates the simulated exchange position, the
    /// cash ledger (with fees), and queues a FastExecData so the generator's
    /// check_for_fills path reconciles the strategy position and live queues.
    async fn exec_fill(
        &mut self,
        order_id: String,
        side: &str,
        qty: f64,
        price: f64,
        ts: u64,
        maker: bool,
    ) {
        let is_buy = side.eq_ignore_ascii_case("buy");
        if let Some(sim) = self.gen.sim() {
            sim.lock().await.apply_fill(is_buy, qty);
        }
        let seq = self.report.maker_fills + self.report.taker_fills + 1;
        self.pending_execs.push_back(FastExecData {
            category: "linear".into(),
            symbol: self.report.symbol.clone(),
            exec_id: format!("EXEC{}", seq),
            exec_price: price,
            exec_qty: qty,
            order_id,
            order_link_id: String::new(),
            side: side.to_string(),
            exec_time: ts,
            seq,
        });
        self.cash += if is_buy { -qty * price } else { qty * price };
        let fee = qty * price * if maker { MAKER_FEE } else { TAKER_FEE };
        self.cash -= fee;
        if maker {
            self.report.maker_fills += 1;
            self.report.maker_notional += qty * price;
            self.report.maker_fees += fee;
        } else {
            self.report.taker_fills += 1;
            self.report.taker_notional += qty * price;
            self.report.taker_fees += fee;
        }
    }
}

/// FIFO queue-position fill simulation. Each order records the volume that
/// was visible ahead of it at its price level when placed (`queue_ahead`):
/// a trade printing *at* the order's price consumes the queue ahead first
/// and only fills the order with the remainder, while a trade printing
/// *through* the level (worse price) means the whole queue was swept and the
/// order fills outright. Orders with unknown queue position fall back to the
/// conservative trade-through fill.
async fn simulate_trade_fills(st: &mut SymState, trade: &WsTrade) {
    let mut volume_left = trade.volume;
    let mut actions: Vec<(bool, usize, String, f64, f64)> = Vec::new();
    let mut buy_ahead_updates: Vec<(usize, f64)> = Vec::new();
    let mut sell_ahead_updates: Vec<(usize, f64)> = Vec::new();

    // Buy orders fill when the trade price is at or below the order price.
    let mut candidates: Vec<(usize, String, f64, f64, Option<f64>)> = Vec::new();
    for (i, o) in st.gen.live_buys_orders.iter().enumerate() {
        if trade.price <= o.price {
            let remaining = (o.qty - o.filled_qty).max(0.0);
            if remaining > 0.0 {
                candidates.push((i, o.order_id.clone(), o.price, remaining, o.queue_ahead));
            }
        }
    }
    for (level, order_id, price, remaining, ahead) in candidates {
        if volume_left <= 0.0 {
            break;
        }
        let qty = match ahead {
            Some(a) => {
                if (trade.price - price).abs() <= f64::EPSILON {
                    // Trade at our level: the queue ahead is consumed first.
                    let consumed = a.min(volume_left);
                    let new_ahead = a - consumed;
                    volume_left -= consumed;
                    buy_ahead_updates.push((level, new_ahead));
                    if new_ahead <= 0.0 {
                        remaining.min(volume_left)
                    } else {
                        0.0
                    }
                } else {
                    // Trade printed through the level: full sweep.
                    remaining.min(volume_left)
                }
            }
            None => remaining.min(volume_left),
        };
        if qty > 0.0 {
            volume_left -= qty;
            actions.push((true, level, order_id, price, qty));
        }
    }

    // Sell orders fill when the trade price is at or above the order price.
    let mut candidates: Vec<(usize, String, f64, f64, Option<f64>)> = Vec::new();
    for (i, o) in st.gen.live_sells_orders.iter().enumerate() {
        if trade.price >= o.price {
            let remaining = (o.qty - o.filled_qty).max(0.0);
            if remaining > 0.0 {
                candidates.push((i, o.order_id.clone(), o.price, remaining, o.queue_ahead));
            }
        }
    }
    for (level, order_id, price, remaining, ahead) in candidates {
        if volume_left <= 0.0 {
            break;
        }
        let qty = match ahead {
            Some(a) => {
                if (trade.price - price).abs() <= f64::EPSILON {
                    let consumed = a.min(volume_left);
                    let new_ahead = a - consumed;
                    volume_left -= consumed;
                    sell_ahead_updates.push((level, new_ahead));
                    if new_ahead <= 0.0 {
                        remaining.min(volume_left)
                    } else {
                        0.0
                    }
                } else {
                    remaining.min(volume_left)
                }
            }
            None => remaining.min(volume_left),
        };
        if qty > 0.0 {
            volume_left -= qty;
            actions.push((false, level, order_id, price, qty));
        }
    }

    for (level, new_ahead) in buy_ahead_updates {
        st.gen.live_buys_orders[level].queue_ahead = Some(new_ahead);
    }
    for (level, new_ahead) in sell_ahead_updates {
        st.gen.live_sells_orders[level].queue_ahead = Some(new_ahead);
    }

    for (is_buy, level, order_id, price, qty) in actions {
        st.exec_fill(
            order_id,
            if is_buy { "Buy" } else { "Sell" },
            qty,
            price,
            trade.timestamp,
            true,
        )
        .await;
        st.report.fill_by_level[level] += 1;
    }
}

/// Applies one recorded book delta to the simulated symbol state: book
/// update, pending market-order execution, feature update, and grid refresh.
async fn on_book_event(
    st: &mut SymState,
    bids: Vec<Bid>,
    asks: Vec<Ask>,
    ts: u64,
    bba: bool,
    config: &Config,
) {
    if bba {
        st.book.update_bba(bids, asks, ts);
    } else {
        st.book.update(bids, asks, ts);
    }
    st.report.book_updates += 1;

    // Funding accrual on held inventory between updates (continuous
    // approximation of the discrete funding interval).
    let dt_hours = if st.last_fund_ts > 0 && ts > st.last_fund_ts {
        (ts - st.last_fund_ts) as f64 / 3_600_000.0
    } else {
        0.0
    };
    if dt_hours > 0.0 && config.strategy.funding_bps_per_hour != 0.0 {
        let accrual = funding_pnl(
            st.gen.position,
            st.book.get_mid_price(),
            config.strategy.funding_bps_per_hour,
            dt_hours,
        );
        st.cash += accrual;
        st.report.funding_paid += -accrual;
    }
    st.last_fund_ts = ts;

    // Execute pending market orders (inventory rebalances) at the current
    // best prices.
    let pending = match st.gen.sim() {
        Some(sim) => sim.lock().await.take_pending_market(),
        None => Vec::new(),
    };
    for (order_id, is_buy, qty) in pending {
        let price = if is_buy {
            st.book.best_ask.price
        } else {
            st.book.best_bid.price
        };
        if price <= 0.0 || !price.is_finite() {
            // Book not ready yet; requeue for the next update.
            if let Some(sim) = st.gen.sim() {
                sim.lock().await.pending_market.push((order_id, is_buy, qty));
            }
            continue;
        }
        st.exec_fill(
            order_id,
            if is_buy { "Buy" } else { "Sell" },
            qty,
            price,
            ts,
            false,
        )
        .await;
    }

    // Feature-engine update, mirroring MarketMaker::update_features.
    let prev_avg = st.engine.avg_trade_price;
    if let (Some(prev_book), Some(prev_trades)) = (&st.prev_book, &st.prev_trades) {
        st.engine.update(
            &st.book,
            prev_book,
            &st.trades,
            prev_trades,
            &prev_avg,
            config.depths.clone(),
        );
    }
    st.prev_book = Some(st.book.clone());
    st.prev_trades = Some(st.trades.clone());

    // Record the latest prediction (as of this update) for the offline
    // horizon evaluation. Capped so very long recordings stay bounded.
    if st.engine.predicted_price > 0.0 {
        st.pred_log.push((ts, st.book.get_mid_price(), st.engine.predicted_price));
        if st.pred_log.len() > 1_000_000 {
            st.pred_log.drain(..500_000);
        }
    }

    st.pnl_path
        .push(st.cash + st.gen.position * st.book.get_mid_price());

    // Replace the grid once the features are warmed up, exactly like the live
    // strategy loop.
    if st.warmup > config.tick_window {
        let executions = std::mem::take(&mut st.pending_execs);
        let private = PrivateData::Bybit(BybitPrivate {
            executions,
            ..Default::default()
        });
        st.gen
            .update_grid(
                private,
                st.engine.skew,
                st.engine.mid_return_vol(),
                st.book.clone(),
                st.report.symbol.clone(),
            )
            .await;
        // Record the visible queue ahead of every freshly placed level for
        // the FIFO fill model.
        st.gen.mark_queue_ahead(&st.book);
        st.report.grid_refreshes = st.gen.grid_refreshes;
    } else {
        st.warmup += 1;
    }
}

/// Applies one recorded trade: buffer it, then simulate maker fills against
/// the live grid.
async fn on_trade_event(st: &mut SymState, trade: WsTrade) {
    st.report.trades += 1;
    st.trades.push_back(trade.clone());
    if st.trades.len() > TRADE_BUFFER_CAP {
        st.trades.pop_front();
    }
    simulate_trade_fills(st, &trade).await;
}

/// Replays the recording at record_path with the strategy parameters from
/// config, returning per-symbol statistics.
pub async fn run(record_path: &str, config: &Config) -> Result<BacktestReport, String> {
    let mut stream = ReplayStream::open(record_path).map_err(|e| e.to_string())?;

    let mut states: HashMap<String, SymState> = HashMap::new();
    for meta in &stream.header.symbols {
        let mut book = LocalBook::new();
        book.tick_size = meta.tick_size;
        book.lot_size = meta.lot_size;
        book.min_order_size = meta.min_order_size;
        book.min_notional = meta.min_notional;
        book.post_only_max = meta.post_only_max;
        book.contract_size = meta.contract_size;

        let asset = config
            .balances
            .iter()
            .find(|(s, _)| s == &meta.symbol)
            .map(|(_, a)| *a)
            .unwrap_or_else(|| {
                eprintln!(
                    "No balance configured for {}; its grids will be zero-sized",
                    meta.symbol
                );
                0.0
            });

        let mut gen = QuoteGenerator::new_sim(
            asset,
            config.leverage,
            config.orders_per_side,
            config.final_order_distance,
            config.rate_limit,
            config.strategy.clone(),
        );
        gen.set_spread(config.bps.get(&meta.symbol).copied().unwrap_or(0.0));

        let report = SymbolReport {
            symbol: meta.symbol.clone(),
            fill_by_level: vec![0; config.orders_per_side],
            ..Default::default()
        };
        states.insert(
            meta.symbol.clone(),
            SymState {
                book,
                trades: VecDeque::with_capacity(TRADE_BUFFER_CAP),
                prev_book: None,
                prev_trades: None,
                engine: Engine::new(config.tick_window, config.strategy.clone()),
                gen,
                warmup: 0,
                pending_execs: VecDeque::new(),
                cash: 0.0,
                pnl_path: Vec::new(),
                last_fund_ts: 0,
                pred_log: Vec::new(),
                report,
            },
        );
    }
    if states.is_empty() {
        return Err("recording header has no symbols".to_string());
    }

    let mut first_ts = u64::MAX;
    let mut last_ts = 0u64;
    let mut total_events: u64 = 0;
    while let Some(ev) = stream.next_event().map_err(|e| e.to_string())? {
        total_events += 1;
        if total_events.is_multiple_of(250_000) {
            eprintln!("backtest: {} events replayed", total_events);
        }
        match ev {
            MarketEvent::Book {
                symbol,
                bids,
                asks,
                timestamp,
                bba,
            } => {
                first_ts = first_ts.min(timestamp);
                last_ts = last_ts.max(timestamp);
                if let Some(st) = states.get_mut(&symbol) {
                    on_book_event(st, bids, asks, timestamp, bba, config).await;
                }
            }
            MarketEvent::Trade { symbol, trade } => {
                first_ts = first_ts.min(trade.timestamp);
                last_ts = last_ts.max(trade.timestamp);
                if let Some(st) = states.get_mut(&symbol) {
                    on_trade_event(st, trade).await;
                }
            }
        }
    }

    let mut symbols = Vec::with_capacity(states.len());
    for st in states.values_mut() {
        // Drain anything that arrived after the last book update so the final
        // position and PnL reflect it: market orders still queued execute at
        // the last known best prices, and unprocessed executions are fed
        // through the same check_for_fills path used live.
        let pending = match st.gen.sim() {
            Some(sim) => sim.lock().await.take_pending_market(),
            None => Vec::new(),
        };
        for (order_id, is_buy, qty) in pending {
            let price = if is_buy {
                st.book.best_ask.price
            } else {
                st.book.best_bid.price
            };
            if price <= 0.0 || !price.is_finite() {
                continue;
            }
            st.exec_fill(
                order_id,
                if is_buy { "Buy" } else { "Sell" },
                qty,
                price,
                st.book.last_update,
                false,
            )
            .await;
        }
        let executions = std::mem::take(&mut st.pending_execs);
        st.gen.check_for_fills(PrivateData::Bybit(BybitPrivate {
            executions,
            ..Default::default()
        }));

        st.report.final_position = st.gen.position;
        st.report.final_mid = st.book.get_mid_price();
        st.report.total_pnl = st.cash + st.gen.position * st.report.final_mid;
        st.report.adverse_ratio = st.gen.adverse_ratio;
        st.report.grid_amends = st.gen.grid_amends;
        st.report.rebalance_count = st.gen.rebalance_count;
        let (pred_evals, pred_hits) = evaluate_predictions(&st.pred_log, &config.strategy);
        st.report.pred_evals = pred_evals;
        st.report.pred_hits = pred_hits;
        let deltas: Vec<f64> = st.pnl_path.windows(2).map(|w| w[1] - w[0]).collect();
        if deltas.len() > 1 {
            let mean = deltas.iter().sum::<f64>() / deltas.len() as f64;
            let var = deltas.iter().map(|d| (d - mean).powi(2)).sum::<f64>()
                / deltas.len() as f64;
            st.report.sharpe = if var > 0.0 { mean / var.sqrt() } else { 0.0 };
        }
        symbols.push(st.report.clone());
    }

    Ok(BacktestReport {
        duration_ms: last_ts.saturating_sub(first_ts),
        symbols,
    })
}
