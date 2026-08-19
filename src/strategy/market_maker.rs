use bybit::WsTrade;
use skeleton::exchanges::exchange::{Client, Exchange, PrivateData};
use skeleton::util::helpers::{Config, StrategyConfig};
use skeleton::util::localorderbook::LocalBook;
use skeleton::{exchanges::exchange::MarketMessage, ss::SharedState};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::db::{FeatureSnapshot, TursoDb};
use crate::features::engine::Engine;
use crate::trader::quote_gen::{QuoteGenerator, SymbolState};

/// The outcome of the portfolio risk check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskDecision {
    Ok,
    Halt,
}

/// Extracts the timestamp of the latest market snapshot.
fn market_time(message: &MarketMessage) -> u64 {
    match message {
        MarketMessage::Bybit(v) => v.time,
        MarketMessage::Binance(v) => v.time,
    }
}

/// Pure risk evaluation: halt when the mark-to-market drawdown exceeds
/// `max_drawdown_frac` of initial equity, the sum of absolute inventory
/// deltas across symbols exceeds `max_portfolio_delta`, or the realized
/// per-second volatility exceeds `max_vol_bps` basis points (the flash-crash
/// defense: Bieganowski & Slepaczuk 2026 show naive makers get picked off in
/// volatility spikes, and Yagi et al. 2023 find OBI strategies withdraw in
/// crashes). Limits of 0 disable the respective check.
pub fn evaluate_risk(
    initial_equity: f64,
    equity: f64,
    portfolio_delta: f64,
    vol_bps: f64,
    strategy: &StrategyConfig,
) -> RiskDecision {
    if initial_equity > 0.0 && strategy.max_drawdown_frac > 0.0 {
        let drawdown = (initial_equity - equity) / initial_equity;
        if drawdown > strategy.max_drawdown_frac {
            return RiskDecision::Halt;
        }
    }
    if strategy.max_portfolio_delta > 0.0 && portfolio_delta > strategy.max_portfolio_delta {
        return RiskDecision::Halt;
    }
    if strategy.max_vol_bps > 0.0 && vol_bps > strategy.max_vol_bps {
        return RiskDecision::Halt;
    }
    RiskDecision::Ok
}

pub struct MarketMaker {
    pub features: HashMap<String, Engine>,
    pub old_books: HashMap<String, Arc<LocalBook>>,
    pub old_trades: HashMap<String, Arc<VecDeque<WsTrade>>>,
    pub curr_trades: HashMap<String, Arc<VecDeque<WsTrade>>>,
    pub prev_avg_trade_price: HashMap<String, f64>,
    pub generators: HashMap<String, QuoteGenerator>,
    pub depths: Vec<usize>,
    pub tick_window: usize,
    /// Number of grid levels per side (telemetry + reporting).
    pub orders_per_side: usize,
    /// Initial account equity (sum of configured balances), for the
    /// mark-to-market drawdown kill switch.
    pub initial_equity: f64,
    /// True once the risk kill switch has tripped; quoting stops.
    pub halted: bool,
    /// Optional path where per-symbol state is saved on shutdown.
    pub state_file: Option<String>,
    /// Current strategy constants (kept in sync by apply_config).
    pub strategy: StrategyConfig,
    /// Turso telemetry database, when configured.
    pub db: Option<TursoDb>,
    /// Database flush cadence in milliseconds.
    pub db_sync_ms: u64,
    last_db_flush: u64,
    /// Last seen grid refresh/amend counters per symbol (drives grid_events).
    last_grid_counters: HashMap<String, (u64, u64)>,
}

impl MarketMaker {
    /// Constructs a new `MarketMaker` instance.
    ///
    /// # Arguments
    ///
    /// * `ss` - The shared state containing information about the markets.
    /// * `assets` - The account balance per symbol.
    /// * `leverage` - The leverage to use for position sizing.
    /// * `orders_per_side` - The number of orders to place on each side.
    /// * `final_order_distance` - The distance of the final order from the mid price.
    /// * `depths` - The depths at which to calculate features.
    /// * `rate_limit` - The per-refresh batch-call limit.
    /// * `tick_window` - The feature lookback window in ticks.
    /// * `strategy` - Strategy constants for the engines and quote generators.
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        ss: SharedState,
        assets: HashMap<String, f64>,
        leverage: f64,
        orders_per_side: usize,
        final_order_distance: f64,
        depths: Vec<usize>,
        rate_limit: u32,
        tick_window: usize,
        strategy: StrategyConfig,
    ) -> Self {
        let initial_equity = assets.values().sum();
        MarketMaker {
            features: MarketMaker::build_features(ss.symbols.clone(), tick_window, strategy.clone()),
            old_books: HashMap::new(),
            old_trades: HashMap::new(),
            curr_trades: HashMap::new(),
            prev_avg_trade_price: HashMap::new(),
            generators: MarketMaker::build_generators(
                ss.clients,
                assets,
                orders_per_side,
                leverage,
                final_order_distance,
                rate_limit,
                strategy.clone(),
            )
            .await,
            depths,
            tick_window,
            orders_per_side,
            initial_equity,
            halted: false,
            state_file: None,
            strategy: strategy.clone(),
            db: None,
            db_sync_ms: 5_000,
            last_db_flush: 0,
            last_grid_counters: HashMap::new(),
        }
    }

    /// Continuously receives and processes shared state snapshots, and
    /// applies hot-reloaded configurations from the watcher channel.
    ///
    /// The first `tick_window` messages are used only to warm up the features;
    /// after that the grid is (re)placed whenever needed. Snapshots are
    /// consumed as fast as they arrive: feature updates are cheap, and an
    /// artificial throttle would only let the channel back up during warmup.
    pub async fn start_loop(
        &mut self,
        mut receiver: UnboundedReceiver<Arc<SharedState>>,
        mut config_rx: UnboundedReceiver<Config>,
    ) {
        let mut warmup_ticks = 0;
        let mut warned_both = false;
        loop {
            tokio::select! {
                data = receiver.recv() => {
                    let Some(data) = data else { break };
                    match data.exchange.as_str() {
                        "bybit" | "binance" => {
                            let ts = market_time(&data.markets[0]);
                            // Update features with the latest market data.
                            self.update_features(data.markets[0].clone(), self.depths.clone());

                            // Replace the grid once the features are warmed up.
                            if warmup_ticks > self.tick_window {
                                if !self.halted {
                                    self.potentially_update(data.private.clone(), data.markets[0].clone())
                                        .await;
                                }
                            } else {
                                warmup_ticks += 1;
                            }

                            // Flush telemetry to Turso on the sync cadence.
                            if self.db.is_some()
                                && ts.saturating_sub(self.last_db_flush) >= self.db_sync_ms
                            {
                                self.flush_db(ts).await;
                                self.last_db_flush = ts;
                            }
                        }
                        "both" => {
                            if !warned_both {
                                eprintln!(
                                    "'both' exchange mode is not yet supported by the strategy loop; \
                                     no orders will be placed"
                                );
                                warned_both = true;
                            }
                        }
                        _ => {
                            panic!("Invalid exchange");
                        }
                    }
                }
                Some(config) = config_rx.recv() => {
                    self.apply_config(&config);
                }
            }
        }
    }

    /// Applies a (re)loaded configuration: per-symbol spreads and strategy
    /// constants for every engine and generator.
    pub fn apply_config(&mut self, config: &Config) {
        for (symbol, generator) in self.generators.iter_mut() {
            match config.bps.get(symbol) {
                Some(spread) => generator.set_spread(*spread),
                None => eprintln!("No spread configured for {}; keeping current", symbol),
            }
            generator.set_strategy(config.strategy.clone());
        }
        for engine in self.features.values_mut() {
            engine.set_strategy(config.strategy.clone());
        }
        self.strategy = config.strategy.clone();
        self.db_sync_ms = config.turso.sync_interval_secs.saturating_mul(1000);
        println!(
            "Config hot-reloaded: {} symbols, strategy updated",
            self.generators.len()
        );
    }

    /// Mark-to-market equity, aggregate inventory exposure, and the worst
    /// per-second volatility across symbols, fed to the pure risk evaluation.
    fn risk_decision(&self) -> RiskDecision {
        let mut equity = self.initial_equity;
        let mut portfolio_delta = 0.0;
        let mut max_vol = 0.0f64;
        for (symbol, generator) in &self.generators {
            if let Some(book) = self.old_books.get(symbol) {
                equity += generator.position * book.get_mid_price();
            }
            portfolio_delta += generator.inventory_delta.abs();
        }
        for engine in self.features.values() {
            max_vol = max_vol.max(engine.mid_return_vol());
        }
        // Per-update vol -> per-second vol (x10) -> basis points (x10000).
        let vol_bps = max_vol * 10.0 * 10_000.0;
        evaluate_risk(self.initial_equity, equity, portfolio_delta, vol_bps, &self.strategy)
    }

    /// Collects the pending telemetry (engine snapshots, fills, grid events)
    /// into one SQL batch and sends it to the Turso database.
    async fn flush_db(&mut self, ts: u64) {
        let mut batch = String::new();

        for (symbol, engine) in &self.features {
            if let Some(mid) = engine.last_mid() {
                batch.push_str(&TursoDb::feature_insert_sql(
                    ts,
                    symbol,
                    &FeatureSnapshot {
                        mid,
                        skew: engine.skew,
                        vol: engine.mid_return_vol(),
                        imb: engine.imbalance_ratio,
                        ofi_scaled: engine.ofi_scaled,
                        trade_imb: engine.trade_imb,
                        voi: engine.voi,
                        regime: engine.imbalance_regime,
                        predicted: engine.predicted_price,
                    },
                ));
            }
        }

        for (symbol, generator) in self.generators.iter_mut() {
            for fill in generator.drain_fills() {
                batch.push_str(&TursoDb::fill_insert_sql(symbol, &fill));
            }
            let (last_refresh, last_amend) = self
                .last_grid_counters
                .get(symbol)
                .copied()
                .unwrap_or((0, 0));
            if generator.grid_refreshes > last_refresh {
                batch.push_str(&TursoDb::grid_insert_sql(
                    ts,
                    symbol,
                    "refresh",
                    self.orders_per_side as i64,
                ));
            }
            if generator.grid_amends > last_amend {
                batch.push_str(&TursoDb::grid_insert_sql(
                    ts,
                    symbol,
                    "amend",
                    self.orders_per_side as i64,
                ));
            }
            self.last_grid_counters
                .insert(symbol.clone(), (generator.grid_refreshes, generator.grid_amends));
        }

        if !batch.is_empty() {
            if let Some(db) = &self.db {
                if let Err(e) = db.execute_batch_raw(batch).await {
                    eprintln!("Turso flush failed: {}", e);
                }
            }
        }
    }

    /// Saves per-symbol position and live orders to the configured state file.
    pub fn save_state(&self) -> Result<(), String> {
        let Some(path) = &self.state_file else {
            return Ok(());
        };
        let snapshot: HashMap<String, SymbolState> = self
            .generators
            .iter()
            .map(|(symbol, generator)| (symbol.clone(), generator.snapshot_state()))
            .collect();
        let json = serde_json::to_string_pretty(&snapshot).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }

    /// Restores previously saved per-symbol state (position and live orders),
    /// if the state file exists.
    pub fn load_state(&mut self) -> Result<(), String> {
        let Some(path) = &self.state_file else {
            return Ok(());
        };
        let Ok(json) = std::fs::read_to_string(path) else {
            return Ok(()); // no previous state; start fresh
        };
        let snapshot: HashMap<String, SymbolState> =
            serde_json::from_str(&json).map_err(|e| e.to_string())?;
        for (symbol, state) in snapshot {
            if let Some(generator) = self.generators.get_mut(&symbol) {
                generator.restore_state(state);
            }
        }
        println!("Restored state for {} symbols", self.generators.len());
        Ok(())
    }

    /// Cancels all open orders, prints the final position and structured
    /// stats per symbol, and persists state. Called on graceful shutdown.
    pub async fn shutdown(&mut self) {
        for (symbol, generator) in self.generators.iter() {
            match generator.cancel_all(symbol).await {
                Ok(_) => println!("Cancelled all orders for {}", symbol),
                Err(_) => eprintln!("Failed to cancel all orders for {}", symbol),
            }
            println!("Final position for {}: {:.6}", symbol, generator.position);
            println!("Stats for {}: {}", symbol, generator.stats_summary());
        }
        // Final telemetry flush before exit.
        if self.db.is_some() {
            let now = skeleton::util::helpers::generate_timestamp();
            self.flush_db(now).await;
        }
        if let Err(e) = self.save_state() {
            eprintln!("Failed to save state: {}", e);
        }
    }

    /// Builds a feature engine per symbol.
    fn build_features(
        symbol: Vec<String>,
        tick_window: usize,
        strategy: StrategyConfig,
    ) -> HashMap<String, Engine> {
        symbol
            .into_iter()
            .map(|v| (v, Engine::new(tick_window, strategy.clone())))
            .collect()
    }

    /// Builds a quote generator per client symbol, setting leverage and
    /// syncing the position with the exchange before quoting starts.
    async fn build_generators(
        clients: HashMap<String, Client>,
        assets: HashMap<String, f64>,
        orders_per_side: usize,
        leverage: f64,
        final_order_distance: f64,
        rate_limit: u32,
        strategy: StrategyConfig,
    ) -> HashMap<String, QuoteGenerator> {
        let mut hash: HashMap<String, QuoteGenerator> = HashMap::new();
        let leverage = leverage.clamp(1.0, 100.0);

        for (symbol, client) in clients {
            // Missing balances produce zero-sized grids rather than panicking.
            let asset = assets.get(&symbol).copied().unwrap_or_else(|| {
                eprintln!(
                    "No balance configured for {}; position sizing will be zero",
                    symbol
                );
                0.0
            });

            match &client {
                Client::Bybit(cl) => match cl.set_leverage(&symbol, leverage as u16).await {
                    Ok(_) => println!("Set leverage for {} to {}", symbol, leverage),
                    Err(e) => eprintln!("Failed to set leverage for {}: {}", symbol, e),
                },
                Client::Binance(cl) => match cl.set_leverage(&symbol, leverage as u16).await {
                    Ok(_) => println!("Set leverage for {} to {}", symbol, leverage),
                    Err(e) => eprintln!("Failed to set leverage for {}: {}", symbol, e),
                },
            }

            let mut generator = QuoteGenerator::new(
                client,
                asset,
                leverage,
                orders_per_side,
                final_order_distance,
                rate_limit,
                strategy.clone(),
            );
            // Reconcile with the exchange's actual position before quoting.
            generator.sync_position(&symbol).await;
            hash.insert(symbol, generator);
        }

        hash
    }

    /// Updates the feature engines from the latest market message.
    fn update_features(&mut self, data: MarketMessage, depth: Vec<usize>) {
        // Both exchange variants share the same (symbol, Arc<book>)/(symbol, Arc<trades>) shape.
        let (trades, books) = match data {
            MarketMessage::Bybit(v) => (v.trades, v.books),
            MarketMessage::Binance(v) => (v.trades, v.books),
        };

        for (k, t) in trades {
            self.curr_trades.insert(k, t);
        }

        for (k, b) in books {
            let Some(feature) = self.features.get_mut(&k) else {
                eprintln!("No feature engine configured for symbol {}", k);
                continue;
            };

            let prev_book = self.old_books.get(&k);
            let prev_trade = self.old_trades.get(&k);
            let prev_avg = self.prev_avg_trade_price.get(&k);
            let curr_trade = self.curr_trades.get(&k);

            if let (Some(book), Some(p_trades), Some(p_avg), Some(curr_trades)) =
                (prev_book, prev_trade, prev_avg, curr_trade)
            {
                feature.update(
                    b.as_ref(),
                    book.as_ref(),
                    curr_trades.as_ref(),
                    p_trades.as_ref(),
                    p_avg,
                    depth.clone(),
                );
            }

            self.old_books.insert(k.clone(), b);
            self.prev_avg_trade_price.insert(k, feature.avg_trade_price);
        }

        // Cheap Arc clones; the underlying buffers are shared.
        self.old_trades = self.curr_trades.clone();
    }

    /// Updates each symbol's quote grid with new market and private data.
    async fn potentially_update(
        &mut self,
        private: HashMap<String, PrivateData>,
        data: MarketMessage,
    ) {
        // Portfolio risk kill switch: halt quoting (and cancel everything)
        // when the configured drawdown or aggregate-inventory limit trips.
        match self.risk_decision() {
            RiskDecision::Halt => {
                self.halted = true;
                eprintln!("RISK HALT: limits breached; cancelling all orders and stopping quoting");
                for (symbol, generator) in &self.generators {
                    let _ = generator.cancel_all(symbol).await;
                }
                return;
            }
            RiskDecision::Ok => {}
        }

        let books = match data {
            MarketMessage::Bybit(v) => v.books,
            MarketMessage::Binance(v) => v.books,
        };

        for (symbol, book) in books {
            let Some(engine) = self.features.get(&symbol) else {
                eprintln!("No feature engine for {}; skipping update", symbol);
                continue;
            };
            let skew = engine.skew;
            let vol = engine.mid_return_vol();
            let Some(symbol_quoter) = self.generators.get_mut(&symbol) else {
                eprintln!("No quote generator for {}; skipping update", symbol);
                continue;
            };

            if let Some(private_data) = private.get(&symbol) {
                symbol_quoter
                    .update_grid(private_data.clone(), skew, vol, (*book).clone(), symbol)
                    .await;
            }
        }
    }

    /// Sets the spread per generator from a symbol-keyed map of basis points.
    pub fn set_spread_toml(&mut self, bps: HashMap<String, f64>) {
        for (symbol, generator) in self.generators.iter_mut() {
            match bps.get(symbol) {
                Some(spread) => generator.set_spread(*spread),
                None => eprintln!(
                    "No spread configured for {}; using the built-in default",
                    symbol
                ),
            }
        }
    }
}
