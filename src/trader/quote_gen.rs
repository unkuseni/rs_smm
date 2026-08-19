use std::{borrow::Cow, collections::VecDeque, sync::Arc};

use binance::api::Binance;
use binance::futures::account::{FuturesAccount, TimeInForce};
use bybit::{
    AmendOrderRequest, BatchAmendRequest, BatchPlaceRequest, Bybit, BybitError, CancelAllRequest,
    Category, FastExecData, OrderRequest, OrderType, PositionManager, PositionRequest, Side,
};
use futures::future::join_all;
use skeleton::{
    exchanges::{
        ex_binance::BinanceClient,
        ex_bybit::BybitClient,
        exchange::{Client, Exchange, PrivateData},
    },
    util::{
        helpers::{geometric_weights, geomspace, nbsqrt, round_step, Round, StrategyConfig},
        localorderbook::LocalBook,
    },
};
use tokio::sync::Mutex;

/// [qty, price, symbol, side] where side is -1 for sell and 1 for buy.
///
/// The `BatchOrder` struct represents an order to be placed or cancelled in a
/// batch operation.
#[derive(Debug, Clone)]
pub struct BatchOrder(f64, f64, String, i32);

impl BatchOrder {
    pub fn new(qty: f64, price: f64, side: i32) -> Self {
        BatchOrder(qty, price, String::new(), side)
    }
}

/// What the strategy should do with the current grid after a book update.
enum GridAction {
    /// Grid is fine; nothing to do.
    None,
    /// Cancel everything and place a fresh grid (empty grid, fills, staleness).
    Replace,
    /// Move the existing levels in place, keeping order ids and therefore
    /// queue priority (the mid moved outside the bounds).
    Amend,
}

/// An in-memory exchange used by the offline backtest harness. Orders are
/// accepted immediately; fills are driven externally by the harness, which
/// applies trade-through fills to resting orders and executes pending market
/// orders against the simulated book's best prices.
pub struct SimExchange {
    /// Signed position (positive = long), updated by every simulated fill.
    pub position: f64,
    next_order_id: u64,
    /// Market orders waiting to be executed at the next book's best prices:
    /// (order_id, is_buy, qty).
    pub pending_market: Vec<(String, bool, f64)>,
}

impl SimExchange {
    pub fn new() -> Self {
        Self {
            position: 0.0,
            next_order_id: 1,
            pending_market: Vec::new(),
        }
    }

    fn next_id(&mut self, prefix: &str) -> String {
        let id = format!("{}{}", prefix, self.next_order_id);
        self.next_order_id += 1;
        id
    }

    /// Applies a fill to the simulated position.
    pub fn apply_fill(&mut self, is_buy: bool, qty: f64) {
        if is_buy {
            self.position += qty;
        } else {
            self.position -= qty;
        }
    }

    /// Takes the pending market orders (executed by the harness at the next
    /// book's best bid/ask).
    pub fn take_pending_market(&mut self) -> Vec<(String, bool, f64)> {
        std::mem::take(&mut self.pending_market)
    }
}

impl Default for SimExchange {
    fn default() -> Self {
        Self::new()
    }
}

/// The exchange-specific order management backend used by `QuoteGenerator`.
///
/// `Sim` never touches the network: the offline backtest harness drives its
/// fills through [`SimExchange`].
enum OrderManagement {
    Bybit(BybitClient),
    Binance(BinanceClient),
    Sim(Arc<Mutex<SimExchange>>),
}

/// The `QuoteGenerator` maintains a live grid of maker orders for one symbol
/// and regenerates it when the market moves out of bounds or a fill occurs.
pub struct QuoteGenerator {
    client: OrderManagement,
    minimum_spread: f64,
    pub live_buys_orders: VecDeque<LiveOrder>,
    pub live_sells_orders: VecDeque<LiveOrder>,
    pub position: f64,
    max_position_usd: f64,
    pub inventory_delta: f64,
    total_order: usize,
    bounds_spread: f64,
    final_order_distance: f64,
    last_update_price: f64,
    initial_limit: u32,
    rate_limit: u32,
    time_limit: u64,
    cancel_limit: u32,
    last_position_sync: u64,
    last_rebalance_ms: u64,
    /// Number of grid placements so far (observability for the backtest
    /// harness and live logs).
    pub grid_refreshes: u64,
    /// Number of in-place grid amendments so far.
    pub grid_amends: u64,
    /// Total matched fills processed (all orders, including partials).
    pub total_fills: u64,
    /// Number of market-order rebalances submitted.
    pub rebalance_count: u64,
    /// Running mean realized/quoted spread ratio across roundtrip pairs.
    /// Below 1.0 means we capture less than the quoted spread: adverse
    /// selection (Bieganowski & Slepaczuk 2026's key maker diagnostic).
    pub adverse_ratio: f64,
    spread_roundtrips: u64,
    /// FIFO queue of unmatched buy fills: (execution price, quote price).
    open_buy_entries: VecDeque<(f64, f64)>,
    /// Highest cumulative fill quantity seen per order id for orders that do
    /// not match a live grid order (market-order rebalances on Binance, where
    /// executions report cumulative quantities). Bounded deque so growth is
    /// capped; old entries fall out with the exchange's rolling window.
    market_fill_seen: VecDeque<(String, f64)>,
    /// Execution ids already processed on increment-style feeds (Bybit/sim).
    /// The exchange keeps a rolling window of recent executions (capacity
    /// 2000), so this deque is capped just above that: ids older than the
    /// cap can never reappear and are dropped without re-count risk.
    seen_exec_ids: VecDeque<String>,
    strategy: StrategyConfig,
}

impl QuoteGenerator {
    /// Creates a new `QuoteGenerator` for the given client and asset.
    ///
    /// * `client` - The exchange client used to place orders.
    /// * `asset` - The account balance (in quote currency) backing this symbol.
    /// * `leverage` - The leverage to use for position sizing.
    /// * `orders_per_side` - The number of orders to place on each side.
    /// * `final_order_distance` - Multiplier of the quote spread defining the furthest grid level.
    /// * `rate_limit` - The number of batch calls allowed before the limit refreshes.
    /// * `strategy` - Strategy constants (weights, spread scaling, rebalancing).
    pub fn new(
        client: Client,
        asset: f64,
        leverage: f64,
        orders_per_side: usize,
        final_order_distance: f64,
        rate_limit: u32,
        strategy: StrategyConfig,
    ) -> Self {
        let trader = match client {
            Client::Bybit(cl) => OrderManagement::Bybit(cl),
            Client::Binance(cl) => OrderManagement::Binance(cl),
        };
        QuoteGenerator::from_management(
            trader,
            asset,
            leverage,
            orders_per_side,
            final_order_distance,
            rate_limit,
            strategy,
        )
    }

    /// Builds a generator backed by an in-memory [`SimExchange`]. Used by the
    /// offline backtest harness; no network calls are made.
    pub fn new_sim(
        asset: f64,
        leverage: f64,
        orders_per_side: usize,
        final_order_distance: f64,
        rate_limit: u32,
        strategy: StrategyConfig,
    ) -> Self {
        QuoteGenerator::from_management(
            OrderManagement::Sim(Arc::new(Mutex::new(SimExchange::new()))),
            asset,
            leverage,
            orders_per_side,
            final_order_distance,
            rate_limit,
            strategy,
        )
    }

    /// Shared constructor over an already-resolved order management backend.
    fn from_management(
        client: OrderManagement,
        asset: f64,
        leverage: f64,
        orders_per_side: usize,
        final_order_distance: f64,
        rate_limit: u32,
        strategy: StrategyConfig,
    ) -> Self {
        QuoteGenerator {
            client,
            live_buys_orders: VecDeque::new(),
            live_sells_orders: VecDeque::new(),
            position: 0.0,
            inventory_delta: 0.0,
            max_position_usd: QuoteGenerator::update_max(asset, leverage),
            total_order: orders_per_side,
            minimum_spread: 0.0,
            bounds_spread: 0.0,
            final_order_distance,
            last_update_price: 0.0,
            initial_limit: rate_limit,
            rate_limit,
            time_limit: 0,
            cancel_limit: rate_limit,
            last_position_sync: 0,
            last_rebalance_ms: 0,
            grid_refreshes: 0,
            grid_amends: 0,
            total_fills: 0,
            rebalance_count: 0,
            adverse_ratio: 0.0,
            spread_roundtrips: 0,
            open_buy_entries: VecDeque::new(),
            market_fill_seen: VecDeque::new(),
            seen_exec_ids: VecDeque::new(),
            strategy,
        }
    }

    /// Replaces the strategy configuration (used by config hot-reload).
    pub fn set_strategy(&mut self, strategy: StrategyConfig) {
        self.strategy = strategy;
    }

    /// Captures the per-symbol state for persistence across restarts.
    pub fn snapshot_state(&self) -> SymbolState {
        SymbolState {
            position: self.position,
            live_buys: self.live_buys_orders.iter().cloned().collect(),
            live_sells: self.live_sells_orders.iter().cloned().collect(),
        }
    }

    /// Restores a previously saved state (position and live order queues).
    pub fn restore_state(&mut self, state: SymbolState) {
        self.position = state.position;
        self.live_buys_orders = state.live_buys.into();
        self.live_sells_orders = state.live_sells.into();
        sort_grid(&mut self.live_buys_orders, 1);
        sort_grid(&mut self.live_sells_orders, -1);
    }

    /// Initializes the FIFO queue-ahead quantity for orders that do not have
    /// one yet, from the book depth at their price level. Used by the backtest
    /// harness after each grid placement.
    pub(crate) fn mark_queue_ahead(&mut self, book: &LocalBook) {
        for order in self.live_buys_orders.iter_mut() {
            if order.queue_ahead.is_none() {
                order.queue_ahead = Some(book.bid_qty_at(order.price).unwrap_or(0.0));
            }
        }
        for order in self.live_sells_orders.iter_mut() {
            if order.queue_ahead.is_none() {
                order.queue_ahead = Some(book.ask_qty_at(order.price).unwrap_or(0.0));
            }
        }
    }

    /// Structured stats summary for shutdown/logging.
    pub fn stats_summary(&self) -> String {
        format!(
            "grid_refreshes={} grid_amends={} fills={} rebalances={} adverse_ratio={:.3} position={:.6}",
            self.grid_refreshes,
            self.grid_amends,
            self.total_fills,
            self.rebalance_count,
            self.adverse_ratio,
            self.position
        )
    }

    /// Marks an execution id as processed. Returns true when the id was
    /// already seen (the fill must be skipped).
    fn exec_seen(&mut self, exec_id: &str) -> bool {
        if self.seen_exec_ids.iter().any(|id| id == exec_id) {
            return true;
        }
        // Bybit's execution deque holds the latest 2000 executions, so ids
        // older than this cap can never be re-delivered.
        if self.seen_exec_ids.len() >= 2048 {
            self.seen_exec_ids.pop_front();
        }
        self.seen_exec_ids.push_back(exec_id.to_string());
        false
    }

    /// Returns the highest cumulative fill quantity seen for an order id.
    fn market_fill_get(&self, order_id: &str) -> Option<f64> {
        self.market_fill_seen
            .iter()
            .find(|(id, _)| id == order_id)
            .map(|(_, qty)| *qty)
    }

    /// Records the highest cumulative fill quantity for an order id.
    fn market_fill_set(&mut self, order_id: String, qty: f64) {
        if let Some(entry) = self.market_fill_seen.iter_mut().find(|(id, _)| *id == order_id) {
            entry.1 = entry.1.max(qty);
            return;
        }
        if self.market_fill_seen.len() >= 2048 {
            self.market_fill_seen.pop_front();
        }
        self.market_fill_seen.push_back((order_id, qty));
    }

    /// Returns the simulated exchange backend when this generator runs in
    /// sim mode; the backtest harness uses it to drive fills.
    pub fn sim(&self) -> Option<Arc<Mutex<SimExchange>>> {
        match &self.client {
            OrderManagement::Sim(sim) => Some(sim.clone()),
            _ => None,
        }
    }

    /// Calculates the maximum position value in USD.
    ///
    /// The asset value is multiplied by the leverage and then reduced by a
    /// safety margin of 7% (0.93 factor) to leave a buffer for adverse moves.
    pub fn update_max(asset: f64, leverage: f64) -> f64 {
        let safety_margin: f64 = 0.93;
        (asset * leverage) * safety_margin
    }

    /// Sets the preferred (minimum) spread in basis points.
    pub fn set_spread(&mut self, spread_in_bps: f64) {
        self.minimum_spread = spread_in_bps;
    }

    /// Updates the inventory delta, the position's exposure as a ratio of the
    /// maximum position value in USD.
    pub fn inventory_delta(&mut self, book: &LocalBook) {
        self.inventory_delta = (self.position * book.get_mid_price()) / self.max_position_usd;
    }

    /// Reconciles the local position with the exchange's reported position.
    pub async fn sync_position(&mut self, symbol: &str) {
        match self.client.sync_position(symbol).await {
            Ok(pos) => {
                if pos != self.position {
                    println!(
                        "Position synced for {}: {:.6} -> {:.6}",
                        symbol, self.position, pos
                    );
                    self.position = pos;
                }
            }
            Err(e) => eprintln!("Failed to sync position for {}: {}", symbol, e),
        }
    }

    /// Cancels all open orders for this generator's symbol.
    pub async fn cancel_all(&self, symbol: &str) -> Result<Vec<LiveOrder>, ()> {
        self.client.cancel_all(symbol).await
    }

    /// Calculates the spread clipped to a minimum (the preferred spread) and a
    /// maximum of 3.7 times the minimum, then widens it for volatility.
    ///
    /// The volatility term multiplies the base spread by
    /// (1 + vol_spread_scaling * vol), where vol is the standard deviation of
    /// mid returns over the feature window. The engine's vol is per-update;
    /// updates run on a ~10ms cadence, so it is rescaled to per-second
    /// volatility (x10) before applying the configured scaling. Wider quotes
    /// in volatile regimes protect against adverse selection (Bieganowski &
    /// Slepaczuk 2026 show fixed spreads are picked off during volatility
    /// spikes).
    ///
    /// 1 bps = 0.01% = 0.0001
    fn adjusted_spread(&self, preferred_spread: f64, book: &LocalBook, vol: f64) -> f64 {
        let min_spread = {
            if preferred_spread == 0.0 {
                bps_to_decimal(27.0) * book.get_mid_price()
            } else {
                bps_to_decimal(preferred_spread) * book.get_mid_price()
            }
        };
        let base = book.get_spread().clip(min_spread, min_spread * 3.7);
        // Per-second volatility under the documented 1 tick = 10ms cadence.
        let vol_per_second = vol * 10.0;
        if vol_per_second.is_finite() && vol_per_second > 0.0 {
            base * (1.0 + self.strategy.vol_spread_scaling * vol_per_second)
        } else {
            base
        }
    }

    /// Updates the bounds spread used to decide when the grid is out of
    /// bounds. Uses the same volatility widening as the quoted spread so a
    /// vol-widened grid is not immediately declared out of bounds (less
    /// churn in volatile regimes).
    fn bounds_spread(&mut self, spread: f64, last_update_price: f64, book: &LocalBook, vol: f64) {
        let min_spread = {
            if spread == 0.0 {
                bps_to_decimal(27.0) * last_update_price
            } else {
                bps_to_decimal(spread) * last_update_price
            }
        };
        let base = book.get_spread().clip(min_spread, min_spread * 3.7);
        let vol_per_second = vol * 10.0;
        self.bounds_spread = if vol_per_second.is_finite() && vol_per_second > 0.0 {
            base * (1.0 + self.strategy.vol_spread_scaling * vol_per_second)
        } else {
            base
        };
    }

    /// Generates a grid of quotes based on the current book, skew, and inventory.
    ///
    /// The grid is built around the microprice (or the mid price, per config),
    /// skewed toward the side with buying pressure, and adjusted for inventory
    /// to avoid over-exposure in either direction (Avellaneda-Stoikov style
    /// inventory control).
    fn generate_quotes(
        &mut self,
        symbol: String,
        book: &LocalBook,
        skew: f64,
        vol: f64,
    ) -> Vec<BatchOrder> {
        // The microprice adjusts the mid for bid/ask imbalance (Stoikov's
        // estimator), so anchoring here lets the grid tilt with order-book
        // pressure before the skew term even applies.
        let start = if self.strategy.use_microprice_anchor {
            book.get_microprice(None)
        } else {
            book.get_mid_price()
        };
        // An empty or crossed book has no anchor to quote around; geomspace
        // would panic on non-positive bounds, so skip placement entirely.
        if !start.is_finite() || start <= 0.0 || self.total_order == 0 {
            return Vec::new();
        }
        let preferred_spread = self.minimum_spread;
        let base_spread = self.adjusted_spread(preferred_spread, book, vol);

        // Avellaneda-Stoikov reservation pricing (enabled by as_gamma > 0):
        // center quotes at r = s - q*gamma*sigma^2*(T-t) and floor the spread
        // at gamma*sigma^2*(T-t) + (2/gamma)*ln(1+gamma/kappa). Falls back to
        // the heuristic inventory term when disabled.
        let (start, curr_spread) = if self.strategy.as_gamma > 0.0 {
            let sigma = (vol * 10.0).max(1e-9);
            let tau = self.strategy.as_horizon_secs;
            let gamma = self.strategy.as_gamma;
            let kappa = self.strategy.as_kappa.max(1e-9);
            let res_shift = -gamma * self.position * sigma * sigma * tau;
            let as_spread =
                gamma * sigma * sigma * tau + (2.0 / gamma) * (1.0 + gamma / kappa).ln();
            (
                start + res_shift,
                base_spread.max(as_spread),
            )
        } else {
            (start, base_spread)
        };
        let half_spread = curr_spread / 2.0;
        let notional = book.min_notional;

        // Correct the market skew by the current inventory to avoid building
        // up too large a position in one direction. The heuristic term only
        // applies when the AS reservation price is disabled.
        let inventory_factor = nbsqrt(self.inventory_delta);
        let skew_factor = skew * (1.0 - inventory_factor.abs());
        let inventory_adjustment = if self.strategy.as_gamma > 0.0 {
            0.0
        } else {
            -self.strategy.inventory_adjustment * inventory_factor
        };
        let combined_skew = skew_factor + inventory_adjustment;
        let final_skew = combined_skew.clip(-1.0, 1.0);

        let mut orders = if final_skew >= 0.0 {
            self.positive_skew_orders(
                half_spread,
                curr_spread,
                start,
                final_skew.abs(),
                notional,
                book,
            )
        } else {
            self.negative_skew_orders(
                half_spread,
                curr_spread,
                start,
                final_skew.abs(),
                notional,
                book,
            )
        };

        // Assign the symbol, clamp prices away from the best bid/ask so the
        // orders never cross the book (maker-only), round to tick size, and
        // drop anything invalid.
        for order in orders.iter_mut() {
            order.2 = symbol.clone();
        }
        orders.retain(|o| {
            o.0.is_finite() && o.0 > 0.0 && o.1.is_finite() && o.1 > 0.0 && (o.0 * o.1) > notional
        });
        for order in orders.iter_mut() {
            if order.3 < 0 {
                order.1 = order.1.max(book.best_ask.price + book.tick_size);
                order.1 = round_price(book, order.1, -1);
            } else {
                order.1 = order.1.min(book.best_bid.price - book.tick_size);
                order.1 = round_price(book, order.1, 1);
            }
        }

        orders
    }

    /// Generates orders for a positive (buy-heavy) skew: the best bid is
    /// pushed closer to the mid price.
    fn positive_skew_orders(
        &self,
        half_spread: f64,
        curr_spread: f64,
        start: f64,
        aggression: f64,
        notional: f64,
        book: &LocalBook,
    ) -> Vec<BatchOrder> {
        let best_bid = start - (half_spread * (1.0 - aggression.sqrt()));
        let best_ask = best_bid + curr_spread;

        // The range of prices for order placement; keep endpoints strictly positive.
        let end = curr_spread * self.final_order_distance;
        let bid_end = (best_bid - end).max(book.tick_size.max(f64::MIN_POSITIVE));
        let ask_end = (best_ask + end).max(book.tick_size.max(f64::MIN_POSITIVE));

        // Geometric distribution of prices: denser near the mid price.
        let bid_prices = geomspace(best_bid, bid_end, self.total_order);
        let mut ask_prices = geomspace(ask_end, best_ask, self.total_order);
        ask_prices.reverse();

        let clipped_r = aggression.clip(self.strategy.aggression_min, self.strategy.aggression_max);

        // Buy sizes scale down as inventory builds; stop buying entirely above 0.5.
        let bid_sizes = if self.inventory_delta >= 0.5 {
            vec![]
        } else {
            let max_buy_qty =
                (self.max_position_usd / 2.0) - (self.position * book.get_mid_price());
            let size_weights = geometric_weights(clipped_r, self.total_order, true);
            let sizes: Vec<f64> = size_weights
                .iter()
                .map(|w| (w * max_buy_qty).max(0.0))
                .collect();
            sizes
        };

        // Sell sizes scale up as inventory builds; stop selling entirely below -0.5.
        let ask_sizes = if self.inventory_delta <= -0.5 {
            vec![]
        } else {
            let max_sell_qty =
                (self.max_position_usd / 2.0) + (self.position * book.get_mid_price());
            let size_weights = geometric_weights(0.37, self.total_order, false);
            let mut sizes: Vec<f64> = size_weights
                .iter()
                .map(|w| (w * max_sell_qty).max(0.0))
                .collect();
            sizes.reverse();
            sizes
        };

        let mut orders = vec![];
        for (i, bid) in bid_prices.iter().enumerate() {
            if !bid_sizes.is_empty() {
                orders.push(BatchOrder::new(
                    round_size(bid_sizes[i] / *bid, book).min(book.post_only_max),
                    *bid,
                    1,
                ));
            }
            if !ask_sizes.is_empty() {
                orders.push(BatchOrder::new(
                    round_size(ask_sizes[i] / ask_prices[i], book).min(book.post_only_max),
                    ask_prices[i],
                    -1,
                ));
            }
        }

        orders.retain(|o| (o.0 * o.1) > notional);
        orders
    }

    /// Generates orders for a negative (sell-heavy) skew: the best ask is
    /// pushed closer to the mid price. Mirrors `positive_skew_orders`.
    fn negative_skew_orders(
        &self,
        half_spread: f64,
        curr_spread: f64,
        start: f64,
        aggression: f64,
        notional: f64,
        book: &LocalBook,
    ) -> Vec<BatchOrder> {
        let best_ask = start + (half_spread * (1.0 - aggression.sqrt()));
        let best_bid = best_ask - curr_spread;

        let end = curr_spread * self.final_order_distance;
        let bid_end = (best_bid - end).max(book.tick_size.max(f64::MIN_POSITIVE));
        let ask_end = (best_ask + end).max(book.tick_size.max(f64::MIN_POSITIVE));

        let bid_prices = geomspace(best_bid, bid_end, self.total_order);
        let mut ask_prices = geomspace(ask_end, best_ask, self.total_order);
        ask_prices.reverse();

        let clipped_r = aggression.clip(self.strategy.aggression_min, self.strategy.aggression_max);

        let bid_sizes = if self.inventory_delta >= 0.5 {
            vec![]
        } else {
            let max_bid_qty =
                (self.max_position_usd / 2.0) - (self.position * book.get_mid_price());
            let size_weights = geometric_weights(0.37, self.total_order, true);
            let sizes: Vec<f64> = size_weights
                .iter()
                .map(|w| (w * max_bid_qty).max(0.0))
                .collect();
            sizes
        };

        let ask_sizes = if self.inventory_delta <= -0.5 {
            vec![]
        } else {
            let max_sell_qty =
                (self.max_position_usd / 2.0) + (self.position * book.get_mid_price());
            let size_weights = geometric_weights(clipped_r, self.total_order, false);
            let mut sizes: Vec<f64> = size_weights
                .iter()
                .map(|w| (w * max_sell_qty).max(0.0))
                .collect();
            sizes.reverse();
            sizes
        };

        let mut orders = vec![];
        for (i, bid) in bid_prices.iter().enumerate() {
            if !bid_sizes.is_empty() {
                orders.push(BatchOrder::new(
                    round_size(bid_sizes[i] / *bid, book).min(book.post_only_max),
                    *bid,
                    1,
                ));
            }
            if !ask_sizes.is_empty() {
                orders.push(BatchOrder::new(
                    round_size(ask_sizes[i] / ask_prices[i], book).min(book.post_only_max),
                    ask_prices[i],
                    -1,
                ));
            }
        }

        orders.retain(|o| (o.0 * o.1) > notional);
        orders
    }

    /// Submits a reducing market order when the inventory exposure exceeds
    /// the configured threshold (default 0.45 of max position), cutting half
    /// the excess. At most one rebalance per cooldown window (default 30s).
    async fn maybe_rebalance(&mut self, book: &LocalBook, symbol: &str) {
        // Recompute the exposure from the current position and mid so the
        // decision never uses a stale value from the previous grid cycle.
        self.inventory_delta(book);
        let delta = self.inventory_delta;
        let threshold = self.strategy.rebalance_threshold;
        if threshold <= 0.0 || delta.abs() < threshold {
            return;
        }
        let now = book.last_update;
        if self.last_rebalance_ms != 0
            && now.saturating_sub(self.last_rebalance_ms) < self.strategy.rebalance_cooldown_ms
        {
            return;
        }
        let mid = book.get_mid_price();
        if mid <= 0.0 || self.max_position_usd <= 0.0 {
            return;
        }

        let excess = delta.abs() - 0.5 * threshold;
        let qty = round_size((excess * self.max_position_usd / mid).max(0.0), book);
        if qty <= 0.0 || qty * mid < book.min_notional {
            return;
        }
        if self.rate_limit <= 1 {
            eprintln!("Rate limit exhausted; skipping rebalance for {}", symbol);
            return;
        }

        // Short inventory is reduced by buying; long inventory by selling.
        let buy = delta < 0.0;
        match self.client.place_market_order(symbol, buy, qty).await {
            Ok(()) => {
                self.rate_limit = self.rate_limit.saturating_sub(1);
                self.last_rebalance_ms = now;
                self.rebalance_count += 1;
                println!(
                    "Rebalancing {}: {} {:.6} at market (inventory delta {:.4})",
                    symbol,
                    if buy { "buy" } else { "sell" },
                    qty,
                    delta
                );
            }
            Err(e) => eprintln!("Failed to rebalance {}: {}", symbol, e),
        }
    }

    /// Sends a batch of orders to the exchange, splitting into chunks of 10.
    async fn send_batch_orders(&mut self, orders: Vec<BatchOrder>) {
        for order_chunk in orders.chunks(10) {
            let order_response = self.client.batch_place_order(order_chunk.to_vec()).await;

            self.rate_limit = self.rate_limit.saturating_sub(1);

            match order_response {
                Ok(response) => {
                    let buys = response.first().cloned().unwrap_or_default();
                    let sells = response.get(1).cloned().unwrap_or_default();
                    for buy_order in buys {
                        self.live_buys_orders.push_back(buy_order);
                    }
                    // Buys: best (highest) bid first; sells: best (lowest)
                    // ask first.
                    sort_grid(&mut self.live_buys_orders, 1);
                    for sell_order in sells {
                        self.live_sells_orders.push_back(sell_order);
                    }
                    sort_grid(&mut self.live_sells_orders, -1);
                }
                Err(v) => {
                    eprintln!("Batch order error, {:?}", v);
                }
            }
        }
    }

    /// Processes private execution data and updates the position and live
    /// order queues for any fills.
    ///
    /// Partial fills reduce the live order's remaining quantity and update the
    /// position by the filled increment only. Bybit execution quantities are
    /// per-execution increments while Binance reports cumulative filled
    /// quantity, so each fill delta is derived from the order's `filled_qty`.
    pub(crate) fn check_for_fills(&mut self, data: PrivateData) -> bool {
        // Binance reports cumulative filled quantities; Bybit reports increments.
        let cumulative = matches!(&data, PrivateData::Binance(_));

        let fills = match data {
            PrivateData::Bybit(data) => data.executions,
            PrivateData::Binance(data) => data.into_fastexec(),
        };

        let mut fill_occurred = false;

        for FastExecData {
            order_id,
            exec_id,
            exec_price,
            exec_qty,
            side,
            ..
        } in fills
        {
            // Increment-style feeds (Bybit, sim) report each execution once,
            // but the private-data deque keeps a rolling window, so dedup by
            // execution id. Cumulative feeds (Binance) are idempotent via the
            // filled-quantity deltas and do not need this.
            if !cumulative && self.exec_seen(&exec_id) {
                continue;
            }
            // exec_qty is typed f64 in rs_bybit 0.4 (per-execution increments
            // for Bybit, cumulative fills for Binance via into_fastexec).
            let exec_qty_float = exec_qty;
            if exec_qty_float <= 0.0 {
                continue;
            }

            // Normalize side casing ("Buy"/"BUY"/"buy").
            let is_buy = side.eq_ignore_ascii_case("buy");

            let matched = {
                let queue = if is_buy {
                    &mut self.live_buys_orders
                } else {
                    &mut self.live_sells_orders
                };
                let mut matched = false;
                for i in 0..queue.len() {
                    if queue[i].order_id != order_id {
                        continue;
                    }
                    matched = true;

                    let remaining = (queue[i].qty - queue[i].filled_qty).max(0.0);
                    let delta = if cumulative {
                        (exec_qty_float - queue[i].filled_qty).max(0.0)
                    } else {
                        exec_qty_float
                    }
                    .min(remaining);

                    if delta > 0.0 {
                        fill_occurred = true;
                        self.total_fills += 1;
                        queue[i].filled_qty += delta;
                        if is_buy {
                            self.position += delta;
                            // Open a roundtrip leg for the adverse-selection
                            // monitor.
                            self.open_buy_entries.push_back((exec_price, queue[i].price));
                        } else {
                            self.position -= delta;
                            // Close the oldest buy leg FIFO and compare the
                            // realized spread against the quoted spread.
                            if let Some((buy_exec, buy_quote)) = self.open_buy_entries.pop_front() {
                                let realized = exec_price - buy_exec;
                                let quoted = queue[i].price - buy_quote;
                                if quoted > 0.0 && realized.is_finite() {
                                    let ratio = realized / quoted;
                                    let n = self.spread_roundtrips as f64;
                                    self.adverse_ratio =
                                        self.adverse_ratio * (n / (n + 1.0)) + ratio / (n + 1.0);
                                    self.spread_roundtrips += 1;
                                }
                            }
                        }
                        if is_buy {
                            println!(
                                "Buy order filled: ID {}, Qty {}, New position {:.6}",
                                order_id, delta, self.position
                            );
                        } else {
                            println!(
                                "Sell order filled: ID {}, Qty {}, New position {:.6}",
                                order_id, delta, self.position
                            );
                        }
                    }

                    if queue[i].filled_qty >= queue[i].qty {
                        queue.remove(i);
                    }
                    break;
                }
                matched
            };

            if !matched {
                // A fill with no matching live order is a market order we
                // submitted for inventory rebalancing. Reconcile the position
                // directly; the periodic sync_position keeps any drift in
                // check. Binance reports cumulative fill quantities, so only
                // apply the increment above what we have already seen.
                let delta = if cumulative {
                    let prev = self.market_fill_get(&order_id).unwrap_or(0.0);
                    let d = (exec_qty_float - prev).max(0.0);
                    self.market_fill_set(order_id.clone(), exec_qty_float.max(prev));
                    d
                } else {
                    exec_qty_float
                };
                if delta > 0.0 {
                    fill_occurred = true;
                    if is_buy {
                        self.position += delta;
                    } else {
                        self.position -= delta;
                    }
                    println!(
                        "Market order filled: ID {}, Qty {}, New position {:.6}",
                        order_id, delta, self.position
                    );
                }
            }
        }
        fill_occurred
    }

    /// Classifies the current grid after a book update:
    /// - no live orders -> Replace (fresh placement),
    /// - mid moved outside the bounds -> Amend (move levels in place),
    /// - a fill occurred or the grid is stale -> Replace (sizes changed),
    /// - otherwise -> None.
    async fn out_of_bounds(
        &mut self,
        book: &LocalBook,
        symbol: String,
        private: PrivateData,
    ) -> GridAction {
        let bounds = self.bounds_spread;
        let current_bid_bounds = self.last_update_price - bounds;
        let current_ask_bounds = self.last_update_price + bounds;

        // Force a refresh when the grid has been untouched for the
        // configured staleness window (default 3 minutes).
        let stale = book.last_update.saturating_sub(self.time_limit) > self.strategy.grid_stale_ms;

        let fill_occurred = self.check_for_fills(private);
        self.inventory_delta(book);

        let bounds_check =
            book.mid_price < current_bid_bounds || book.mid_price > current_ask_bounds;

        if self.live_buys_orders.is_empty() && self.live_sells_orders.is_empty() {
            self.last_update_price = book.mid_price;
            return GridAction::Replace;
        }

        if self.last_update_price == 0.0 {
            return GridAction::None;
        }

        if bounds_check {
            // Pure translation of the book: the same levels can be moved.
            return GridAction::Amend;
        }

        if fill_occurred || stale {
            // Risk-reducing cancels are never skipped for rate-limit reasons;
            // the counter is only tracked for observability.
            if self.client.cancel_all(symbol.as_str()).await.is_ok() {
                // cancel-all semantics: everything is gone once this succeeds.
                self.live_buys_orders.clear();
                self.live_sells_orders.clear();
                self.last_update_price = book.mid_price;
                self.cancel_limit = self.cancel_limit.saturating_sub(1);
                return GridAction::Replace;
            }
            self.cancel_limit = self.cancel_limit.saturating_sub(1);
        }

        GridAction::None
    }

    /// Moves the live grid levels to the new quotes without cancelling,
    /// keeping order ids (and therefore queue priority). Supported on Bybit
    /// and in simulation; returns false when the backend cannot amend or the
    /// level counts do not match, in which case the caller replaces the grid.
    async fn amend_grid(&mut self, orders: Vec<BatchOrder>, symbol: &str) -> bool {
        let mut new_buys: Vec<BatchOrder> = Vec::with_capacity(self.total_order);
        let mut new_sells: Vec<BatchOrder> = Vec::with_capacity(self.total_order);
        for order in orders {
            if order.3 > 0 {
                new_buys.push(order);
            } else {
                new_sells.push(order);
            }
        }
        if new_buys.len() != self.live_buys_orders.len()
            || new_sells.len() != self.live_sells_orders.len()
        {
            return false;
        }
        match self
            .client
            .amend_orders(symbol, &self.live_buys_orders, &new_buys, &self.live_sells_orders, &new_sells)
            .await
        {
            Ok(true) => {
                for (i, order) in new_buys.iter().enumerate() {
                    self.live_buys_orders[i].price = order.1;
                    self.live_buys_orders[i].qty = order.0;
                }
                for (i, order) in new_sells.iter().enumerate() {
                    self.live_sells_orders[i].price = order.1;
                    self.live_sells_orders[i].qty = order.0;
                }
                self.grid_amends += 1;
                true
            }
            Ok(false) => false,
            Err(e) => {
                eprintln!("Amend failed for {}: {}", symbol, e);
                false
            }
        }
    }

    /// The core strategy loop: refresh the spread bounds, reset rate limits on
    /// a one-second cadence, periodically re-sync the position with the
    /// exchange, reduce oversized inventory with market orders, and replace
    /// the grid when it goes out of bounds.
    ///
    /// `vol` is the mid-return volatility from the feature engine, used to
    /// widen the quoted spread in volatile regimes.
    pub async fn update_grid(
        &mut self,
        private: PrivateData,
        skew: f64,
        vol: f64,
        book: LocalBook,
        symbol: String,
    ) {
        self.bounds_spread(self.minimum_spread, self.last_update_price, &book, vol);

        // Reset rate-limit counters roughly once per second.
        if self.time_limit > 1 {
            let elapsed = book.last_update.saturating_sub(self.time_limit);
            if elapsed > 1000 {
                self.rate_limit = self.initial_limit;
                self.cancel_limit = self.initial_limit;
            }
        }

        // Reconcile the local position with the exchange on the configured
        // cadence (default 60 seconds).
        if self.last_position_sync == 0
            || book.last_update.saturating_sub(self.last_position_sync) > self.strategy.position_sync_ms
        {
            self.sync_position(&symbol).await;
            self.last_position_sync = book.last_update;
        }

        // Reduce oversized inventory with a market order before refreshing
        // quotes (Cartea & Wang 2020: the joint limit/market policy is where
        // the inventory-controlled alpha comes from).
        self.maybe_rebalance(&book, &symbol).await;

        match self.out_of_bounds(&book, symbol.clone(), private).await {
            GridAction::None => {}
            GridAction::Replace => {
                self.inventory_delta(&book);

                let orders = self.generate_quotes(symbol.clone(), &book, skew, vol);

                if self.rate_limit > 1 {
                    self.grid_refreshes += 1;
                    self.send_batch_orders(orders).await;
                } else {
                    eprintln!(
                        "Rate limit exhausted for {}; skipping grid placement until refresh",
                        symbol
                    );
                }

                self.time_limit = book.last_update;
            }
            GridAction::Amend => {
                let orders = self.generate_quotes(symbol.clone(), &book, skew, vol);
                if self.rate_limit > 1 {
                    if !self.amend_grid(orders, &symbol).await {
                        // Backend cannot amend (or level counts diverged):
                        // fall back to a full replacement.
                        if self.client.cancel_all(&symbol).await.is_ok() {
                            self.live_buys_orders.clear();
                            self.live_sells_orders.clear();
                        }
                        let orders = self.generate_quotes(symbol.clone(), &book, skew, vol);
                        self.grid_refreshes += 1;
                        self.send_batch_orders(orders).await;
                    } else {
                        self.rate_limit = self.rate_limit.saturating_sub(1);
                    }
                }
                self.last_update_price = book.mid_price;
                self.time_limit = book.last_update;
            }
        }
    }
}

/// A live resting order tracked by the strategy.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LiveOrder {
    pub price: f64,
    pub qty: f64,
    pub order_id: String,
    /// Quantity of this order that has already been booked into the position.
    pub filled_qty: f64,
    /// Visible book quantity ahead of this order at its price level when it
    /// was placed, for the backtest's FIFO queue-position fill model.
    /// `None` means unknown; the harness initializes it from the book.
    #[serde(default)]
    pub queue_ahead: Option<f64>,
}

impl LiveOrder {
    pub fn new(price: f64, qty: f64, order_id: String) -> Self {
        LiveOrder {
            price,
            qty,
            order_id,
            filled_qty: 0.0,
            queue_ahead: None,
        }
    }
}

/// Per-symbol state persisted across restarts (see `state_file` in config).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SymbolState {
    pub position: f64,
    pub live_buys: Vec<LiveOrder>,
    pub live_sells: Vec<LiveOrder>,
}

impl PartialEq for LiveOrder {
    fn eq(&self, other: &Self) -> bool {
        self.order_id == other.order_id
    }
}

fn bps_to_decimal(bps: f64) -> f64 {
    bps / 10000.0
}

/// Rounds a price to the book's tick size. Buys round down (away from
/// crossing the ask) and sells round up (away from crossing the bid).
fn round_price(book: &LocalBook, price: f64, side: i32) -> f64 {
    let decimals = book.tick_size.count_decimal_places();
    let pow = 10_f64.powi(decimals as i32);
    if side < 0 {
        (price * pow).ceil() / pow
    } else {
        (price * pow).floor() / pow
    }
}

fn round_size(qty: f64, book: &LocalBook) -> f64 {
    round_step(qty, book.lot_size)
}

/// Sorts a live order grid in place: best bid first (descending price) for
/// buys, best ask first (ascending price) for sells. The backtest harness
/// uses the queue position as the grid-level index.
fn sort_grid(orders: &mut VecDeque<LiveOrder>, side: i32) {
    orders.make_contiguous().sort_by(|a, b| {
        if side > 0 {
            // Buys: descending price, best (highest) bid first.
            b.price
                .partial_cmp(&a.price)
                .unwrap_or(std::cmp::Ordering::Equal)
        } else {
            // Sells: ascending price, best (lowest) ask first.
            a.price
                .partial_cmp(&b.price)
                .unwrap_or(std::cmp::Ordering::Equal)
        }
    });
}

impl OrderManagement {
    /// Queries the exchange for the current signed position of a symbol.
    async fn sync_position(&self, symbol: &str) -> Result<f64, String> {
        match self {
            OrderManagement::Bybit(client) => {
                let pm: PositionManager =
                    Bybit::new(Some(client.key.clone()), Some(client.secret.clone()));
                let req = PositionRequest::new(Category::Linear, Some(symbol), None, None, None);
                match pm.get_info(req).await {
                    Ok(v) => match v.result.list.into_iter().find(|p| p.symbol == symbol) {
                        // PositionInfo.side is Option<Side>; shorts are negative.
                        Some(p) => Ok(if matches!(p.side, Some(Side::Sell)) {
                            -p.size
                        } else {
                            p.size
                        }),
                        None => Ok(0.0),
                    },
                    Err(e) => Err(e.to_string()),
                }
            }
            OrderManagement::Binance(client) => {
                let trader: FuturesAccount =
                    Binance::new(Some(client.key.clone()), Some(client.secret.clone()));
                match trader.position_information(symbol).await {
                    Ok(positions) => Ok(positions.iter().map(|p| p.position_amount).sum()),
                    Err(e) => Err(format!("{}", e)),
                }
            }
            OrderManagement::Sim(sim) => Ok(sim.lock().await.position),
        }
    }

    /// Submits a market order used for inventory rebalancing.
    async fn place_market_order(&self, symbol: &str, buy: bool, qty: f64) -> Result<(), String> {
        match self {
            OrderManagement::Bybit(client) => {
                let trader = client.trader();
                let req = OrderRequest {
                    category: Category::Linear,
                    symbol: Cow::Owned(symbol.to_string()),
                    order_type: OrderType::Market,
                    side: if buy { Side::Buy } else { Side::Sell },
                    qty,
                    price: None,
                    time_in_force: None,
                    ..Default::default()
                };
                trader
                    .place_custom_order(req)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
            OrderManagement::Binance(client) => {
                let api: FuturesAccount =
                    Binance::new(Some(client.key.clone()), Some(client.secret.clone()));
                let result = if buy {
                    api.market_buy(symbol, qty).await
                } else {
                    api.market_sell(symbol, qty).await
                };
                result.map(|_| ()).map_err(|e| format!("{}", e))
            }
            // In simulation the market order is queued and executed by the
            // harness at the next book's best bid/ask.
            OrderManagement::Sim(sim) => {
                let mut sim = sim.lock().await;
                let id = sim.next_id("MARKET");
                sim.pending_market.push((id, buy, qty));
                Ok(())
            }
        }
    }

    /// Moves existing orders to new prices/quantities without cancelling,
    /// keeping their ids and queue priority. Returns Ok(true) when amended;
    /// Ok(false) means the backend has no amend support and the caller
    /// should fall back to cancel-and-replace.
    #[allow(clippy::too_many_arguments)]
    async fn amend_orders(
        &self,
        symbol: &str,
        live_buys: &VecDeque<LiveOrder>,
        new_buys: &[BatchOrder],
        live_sells: &VecDeque<LiveOrder>,
        new_sells: &[BatchOrder],
    ) -> Result<bool, String> {
        match self {
            OrderManagement::Bybit(client) => {
                let trader = client.trader();
                let mut requests: Vec<AmendOrderRequest> = Vec::new();
                for (live, next) in live_buys
                    .iter()
                    .zip(new_buys.iter())
                    .chain(live_sells.iter().zip(new_sells.iter()))
                {
                    requests.push(AmendOrderRequest {
                        category: Category::Linear,
                        symbol: Cow::Owned(symbol.to_string()),
                        order_id: Some(Cow::Owned(live.order_id.clone())),
                        qty: next.0,
                        price: Some(next.1),
                        ..Default::default()
                    });
                }
                let req = BatchAmendRequest {
                    category: Category::Linear,
                    requests,
                };
                trader
                    .batch_amend_order(req)
                    .await
                    .map(|_| true)
                    .map_err(|e| e.to_string())
            }
            // The Binance client has no amend endpoint; fall back to replace.
            OrderManagement::Binance(_) => Ok(false),
            // Simulation keeps the same order ids; the caller updates the
            // local queues with the new prices and quantities.
            OrderManagement::Sim(_) => Ok(true),
        }
    }

    /// Cancels all open orders for a symbol, returning the list of cancelled
    /// order ids where the exchange provides them.
    async fn cancel_all(&self, symbol: &str) -> Result<Vec<LiveOrder>, ()> {
        match self {
            OrderManagement::Bybit(trader) => {
                let client = trader.trader();
                let req = CancelAllRequest {
                    category: Category::Linear,
                    symbol,
                    ..Default::default()
                };
                if let Ok(v) = client.cancel_all_orders(req).await {
                    let arr = v
                        .result
                        .list
                        .into_iter()
                        .map(|d| LiveOrder::new(0.0, 0.0, d.order_id))
                        .collect();
                    Ok(arr)
                } else {
                    Err(())
                }
            }
            OrderManagement::Binance(client) => {
                let trader: FuturesAccount =
                    Binance::new(Some(client.key.clone()), Some(client.secret.clone()));
                match trader.cancel_all_open_orders(symbol).await {
                    // The Binance API does not return the cancelled order list,
                    // so callers treat success as "everything was cancelled".
                    Ok(_) => Ok(vec![]),
                    Err(_) => Err(()),
                }
            }
            // The harness clears the live queues itself; nothing to cancel.
            OrderManagement::Sim(_) => Ok(vec![]),
        }
    }

    /// Places a batch of orders and returns the resulting live orders split
    /// into buy and sell queues.
    async fn batch_place_order(
        &self,
        order_array: Vec<BatchOrder>,
    ) -> Result<Vec<VecDeque<LiveOrder>>, BybitError> {
        match self {
            OrderManagement::Bybit(trader) => {
                let client = trader.trader();
                let od_clone = order_array.clone();

                let mut tracking_sells = vec![];
                let mut order_arr = Vec::with_capacity(order_array.len());
                for (index, BatchOrder(qty, price, symbol, side)) in
                    order_array.into_iter().enumerate()
                {
                    let side = if side < 0 {
                        tracking_sells.push(index);
                        Side::Sell
                    } else {
                        Side::Buy
                    };
                    order_arr.push(OrderRequest {
                        category: Category::Linear,
                        symbol: Cow::Owned(symbol),
                        order_type: bybit::OrderType::Limit,
                        side,
                        qty,
                        price: Some(price),
                        time_in_force: Some(Cow::Borrowed("PostOnly")),
                        ..Default::default()
                    });
                }

                let req = BatchPlaceRequest {
                    category: Category::Linear,
                    requests: order_arr,
                };
                match client.batch_place_order(req).await {
                    Ok(v) => {
                        let mut buy_array = VecDeque::new();
                        let mut sell_array = VecDeque::new();
                        for ((i, d), ext_info) in v
                            .result
                            .list
                            .iter()
                            .enumerate()
                            .zip(v.ret_ext_info.list.iter())
                        {
                            if ext_info.msg == "OK" {
                                let order = LiveOrder::new(
                                    od_clone[i].1,
                                    od_clone[i].0,
                                    d.order_id.to_string(),
                                );
                                if tracking_sells.contains(&i) {
                                    sell_array.push_back(order);
                                } else {
                                    buy_array.push_back(order);
                                }
                            } else {
                                eprintln!("Order {} failed: {}", d.order_id, ext_info.msg);
                            }
                        }
                        Ok(vec![buy_array, sell_array])
                    }
                    Err(v) => Err(v),
                }
            }
            OrderManagement::Binance(client) => {
                // Place the batch concurrently and track each acceptance
                // individually. GTX (Good Till Crossing) is the maker-only
                // time-in-force, the Binance equivalent of post-only.
                let key = client.key.clone();
                let secret = client.secret.clone();
                let placements = order_array.into_iter().map(|BatchOrder(qty, price, symbol, side)| {
                    let key = key.clone();
                    let secret = secret.clone();
                    async move {
                        let api: FuturesAccount = Binance::new(Some(key), Some(secret));
                        let result = if side < 0 {
                            api.limit_sell(symbol, qty, price, TimeInForce::GTX).await
                        } else {
                            api.limit_buy(symbol, qty, price, TimeInForce::GTX).await
                        };
                        (result, qty, price, side)
                    }
                });
                let results = join_all(placements).await;
                let mut buy_array = VecDeque::new();
                let mut sell_array = VecDeque::new();
                for (result, qty, price, side) in results {
                    match result {
                        Ok(v) => {
                            let order = LiveOrder::new(price, qty, v.order_id.to_string());
                            if side < 0 {
                                sell_array.push_back(order);
                            } else {
                                buy_array.push_back(order);
                            }
                        }
                        Err(e) => eprintln!("Binance order failed: {}", e),
                    }
                }
                Ok(vec![buy_array, sell_array])
            }
            OrderManagement::Sim(sim) => {
                // The simulated exchange accepts every order immediately; the
                // harness drives fills from the recorded trade stream.
                let mut sim = sim.lock().await;
                let mut buy_array = VecDeque::new();
                let mut sell_array = VecDeque::new();
                for BatchOrder(qty, price, _symbol, side) in order_array {
                    let id = sim.next_id("SIM");
                    let order = LiveOrder::new(price, qty, id);
                    if side < 0 {
                        sell_array.push_back(order);
                    } else {
                        buy_array.push_back(order);
                    }
                }
                Ok(vec![buy_array, sell_array])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bybit::{Ask, Bid};
    use skeleton::exchanges::ex_bybit::BybitPrivate;

    fn test_book(bids: &[(f64, f64)], asks: &[(f64, f64)]) -> LocalBook {
        let mut book = LocalBook::new();
        book.tick_size = 0.01;
        book.lot_size = 0.001;
        book.min_order_size = 0.001;
        book.min_notional = 1.0;
        book.post_only_max = 1000.0;
        book.update(
            bids.iter().map(|&(p, q)| Bid { price: p, qty: q }).collect(),
            asks.iter().map(|&(p, q)| Ask { price: p, qty: q }).collect(),
            1,
        );
        book
    }

    fn test_gen(strategy: StrategyConfig) -> QuoteGenerator {
        QuoteGenerator::new_sim(
            1000.0, // asset
            10.0,   // leverage
            4,      // orders_per_side
            10.0,   // final_order_distance
            100,    // rate_limit
            strategy,
        )
    }

    #[test]
    fn grid_respects_book_and_notional() {
        let mut gen = test_gen(StrategyConfig {
            use_microprice_anchor: false,
            ..Default::default()
        });
        gen.set_spread(10.0);
        let book = test_book(&[(100.0, 10.0)], &[(100.5, 10.0)]);
        let orders = gen.generate_quotes("BTCUSDT".to_string(), &book, 0.0, 0.0);

        // Four levels per side, all within the book bounds.
        assert_eq!(orders.len(), 8, "expected 2*orders_per_side orders");
        let mid = book.get_mid_price();
        for order in &orders {
            assert!(order.0 > 0.0 && order.1 > 0.0);
            assert!(order.0 * order.1 >= book.min_notional);
            if order.3 < 0 {
                assert!(order.1 > mid, "sell below mid: {}", order.1);
            } else {
                assert!(order.1 < mid, "buy above mid: {}", order.1);
            }
        }
    }

    #[test]
    fn volatility_widens_quote_spread() {
        let mut gen = test_gen(StrategyConfig::default());
        gen.set_spread(10.0);
        let book = test_book(&[(100.0, 10.0)], &[(100.5, 10.0)]);
        let base = gen.adjusted_spread(10.0, &book, 0.0);
        let widened = gen.adjusted_spread(10.0, &book, 0.001);
        assert!(widened > base, "vol should widen the spread: {} vs {}", widened, base);
        // Per-second vol = 0.001 * 10, scaling 200 => +200%.
        assert!((widened / base - 3.0).abs() < 1e-9);
    }

    #[test]
    fn microprice_anchor_tilts_bid_up_on_buy_pressure() {
        // Heavy bid side: the microprice sits above the mid. The spread is
        // set wide enough that the anchored best bid stays below the book's
        // best bid (post-only clamping would otherwise equalize the grids).
        let book = test_book(&[(100.0, 100.0)], &[(100.1, 1.0)]);

        let mut anchored = test_gen(StrategyConfig {
            use_microprice_anchor: true,
            ..Default::default()
        });
        anchored.set_spread(30.0);
        let anchored_orders = anchored.generate_quotes("BTCUSDT".to_string(), &book, 0.0, 0.0);

        let mut mid_anchored = test_gen(StrategyConfig {
            use_microprice_anchor: false,
            ..Default::default()
        });
        mid_anchored.set_spread(30.0);
        let mid_orders = mid_anchored.generate_quotes("BTCUSDT".to_string(), &book, 0.0, 0.0);

        let best_bid = |orders: &[BatchOrder]| -> f64 {
            orders.iter().filter(|o| o.3 > 0).map(|o| o.1).fold(0.0, f64::max)
        };
        assert!(
            best_bid(&anchored_orders) > best_bid(&mid_orders),
            "microprice-anchored best bid should be closer to the book"
        );
    }

    #[tokio::test]
    async fn rebalance_submits_reducing_market_order() {
        let mut gen = test_gen(StrategyConfig::default());
        let book = test_book(&[(100.0, 10.0)], &[(100.5, 10.0)]);
        // Long position equivalent to 0.6 of max exposure: beyond the
        // 0.45 threshold. maybe_rebalance recomputes the delta from the
        // position and book, so the position is what the test must set.
        gen.max_position_usd = 9300.0;
        gen.position = 0.6 * gen.max_position_usd / book.get_mid_price();
        gen.maybe_rebalance(&book, "BTCUSDT").await;

        let sim = gen.sim().expect("sim backend");
        let pending = sim.lock().await.take_pending_market();
        assert_eq!(pending.len(), 1, "expected one rebalance order");
        let (_, is_buy, qty) = &pending[0];
        assert!(!is_buy, "long inventory should be reduced with a sell");
        assert!(*qty > 0.0);
    }

    #[test]
    fn duplicate_executions_are_applied_once() {
        use skeleton::exchanges::ex_bybit::BybitPrivate;

        let mut gen = test_gen(StrategyConfig::default());
        let _book = test_book(&[(100.0, 10.0)], &[(100.5, 10.0)]);
        gen.live_buys_orders
            .push_back(LiveOrder::new(100.0, 10.0, "O1".to_string()));

        let private = || {
            let mut executions = VecDeque::new();
            executions.push_back(FastExecData {
                category: "linear".into(),
                symbol: "BTCUSDT".into(),
                exec_id: "E1".into(),
                exec_price: 100.0,
                exec_qty: 3.0,
                order_id: "O1".into(),
                order_link_id: String::new(),
                side: "Buy".into(),
                exec_time: 1,
                seq: 1,
            });
            PrivateData::Bybit(BybitPrivate {
                executions,
                ..Default::default()
            })
        };

        assert!(gen.check_for_fills(private()));
        assert!((gen.position - 3.0).abs() < 1e-9);
        // The same execution re-delivered through the exchange's rolling
        // window must not be counted again.
        assert!(!gen.check_for_fills(private()));
        assert!(
            (gen.position - 3.0).abs() < 1e-9,
            "position double-counted: {}",
            gen.position
        );
    }

    #[tokio::test]
    async fn rebalance_respects_cooldown_and_threshold() {
        let mut gen = test_gen(StrategyConfig::default());
        let book = test_book(&[(100.0, 10.0)], &[(100.5, 10.0)]);
        gen.max_position_usd = 9300.0;

        // Below threshold: nothing submitted.
        gen.position = 0.3 * gen.max_position_usd / book.get_mid_price();
        gen.maybe_rebalance(&book, "BTCUSDT").await;
        assert!(gen.sim().unwrap().lock().await.pending_market.is_empty());

        // Above threshold: one order, then the cooldown blocks a second one.
        gen.position = 0.6 * gen.max_position_usd / book.get_mid_price();
        gen.maybe_rebalance(&book, "BTCUSDT").await;
        gen.maybe_rebalance(&book, "BTCUSDT").await;
        assert_eq!(
            gen.sim().unwrap().lock().await.pending_market.len(),
            1,
            "cooldown should block the second rebalance"
        );
    }

    fn empty_private() -> PrivateData {
        PrivateData::Bybit(BybitPrivate::default())
    }

    #[tokio::test]
    async fn grid_action_machine() {
        let mut gen = test_gen(StrategyConfig::default());
        let book = test_book(&[(100.0, 10.0)], &[(100.5, 10.0)]);

        // Empty grid -> Replace.
        assert!(matches!(
            gen.out_of_bounds(&book, "BTCUSDT".to_string(), empty_private())
                .await,
            GridAction::Replace
        ));

        // Populated grid within bounds -> None.
        gen.live_buys_orders
            .push_back(LiveOrder::new(99.9, 1.0, "B1".to_string()));
        gen.live_sells_orders
            .push_back(LiveOrder::new(100.6, 1.0, "S1".to_string()));
        gen.last_update_price = book.mid_price;
        gen.bounds_spread = 0.5;
        assert!(matches!(
            gen.out_of_bounds(&book, "BTCUSDT".to_string(), empty_private())
                .await,
            GridAction::None
        ));

        // Mid moved outside the bounds -> Amend.
        let moved = test_book(&[(101.0, 10.0)], &[(101.5, 10.0)]);
        assert!(matches!(
            gen.out_of_bounds(&moved, "BTCUSDT".to_string(), empty_private())
                .await,
            GridAction::Amend
        ));

        // A fill within bounds -> Replace.
        let mut executions = VecDeque::new();
        executions.push_back(FastExecData {
            category: "linear".into(),
            symbol: "BTCUSDT".into(),
            exec_id: "E9".into(),
            exec_price: 99.9,
            exec_qty: 1.0,
            order_id: "B1".into(),
            order_link_id: String::new(),
            side: "Buy".into(),
            exec_time: 2,
            seq: 9,
        });
        let private = PrivateData::Bybit(BybitPrivate {
            executions,
            ..Default::default()
        });
        assert!(matches!(
            gen.out_of_bounds(&book, "BTCUSDT".to_string(), private).await,
            GridAction::Replace
        ));
    }

    #[test]
    fn as_gamma_shifts_grid_against_long_inventory() {
        let book = test_book(&[(100.0, 10.0)], &[(100.5, 10.0)]);

        let mut as_gen = test_gen(StrategyConfig {
            as_gamma: 100_000.0,
            as_horizon_secs: 10.0,
            as_kappa: 1.0,
            ..Default::default()
        });
        // A moderate long position: large enough to tilt the reservation,
        // small enough that the buy side still has size budget.
        as_gen.set_spread(10.0);
        as_gen.position = 10.0;
        as_gen.inventory_delta = 10.0 * 100.25 / (1000.0 * 10.0 * 0.93);
        let as_orders = as_gen.generate_quotes("BTCUSDT".to_string(), &book, 0.0, 0.0001);

        let mut plain = test_gen(StrategyConfig::default());
        plain.set_spread(10.0);
        plain.position = 10.0;
        plain.inventory_delta = 10.0 * 100.25 / (1000.0 * 10.0 * 0.93);
        let plain_orders = plain.generate_quotes("BTCUSDT".to_string(), &book, 0.0, 0.0001);

        let best_bid = |orders: &[BatchOrder]| -> f64 {
            orders.iter().filter(|o| o.3 > 0).map(|o| o.1).fold(0.0, f64::max)
        };
        // A long position with the AS reservation price should quote lower
        // bids (reservation below the mid).
        assert!(
            best_bid(&as_orders) < best_bid(&plain_orders),
            "AS reservation should pull quotes down for a long position"
        );
    }

    #[test]
    fn adverse_selection_ratio_tracks_roundtrips() {
        let mut gen = test_gen(StrategyConfig::default());
        gen.live_buys_orders
            .push_back(LiveOrder::new(100.0, 10.0, "B1".to_string()));
        gen.live_sells_orders
            .push_back(LiveOrder::new(101.0, 10.0, "S1".to_string()));

        let buy_exec = || {
            let mut executions = VecDeque::new();
            executions.push_back(FastExecData {
                category: "linear".into(),
                symbol: "BTCUSDT".into(),
                exec_id: "E1".into(),
                exec_price: 100.0,
                exec_qty: 10.0,
                order_id: "B1".into(),
                order_link_id: String::new(),
                side: "Buy".into(),
                exec_time: 1,
                seq: 1,
            });
            PrivateData::Bybit(BybitPrivate {
                executions,
                ..Default::default()
            })
        };
        let sell_exec = || {
            let mut executions = VecDeque::new();
            executions.push_back(FastExecData {
                category: "linear".into(),
                symbol: "BTCUSDT".into(),
                exec_id: "E2".into(),
                exec_price: 100.5,
                exec_qty: 10.0,
                order_id: "S1".into(),
                order_link_id: String::new(),
                side: "Sell".into(),
                exec_time: 2,
                seq: 2,
            });
            PrivateData::Bybit(BybitPrivate {
                executions,
                ..Default::default()
            })
        };

        gen.check_for_fills(buy_exec());
        gen.check_for_fills(sell_exec());

        // Realized spread 0.5 vs quoted spread 1.0 -> ratio 0.5.
        assert!((gen.adverse_ratio - 0.5).abs() < 1e-9);
    }

    #[test]
    fn state_roundtrips_through_json() {
        let mut gen = test_gen(StrategyConfig::default());
        gen.position = 3.5;
        gen.live_buys_orders
            .push_back(LiveOrder::new(99.5, 2.0, "B1".to_string()));
        gen.live_sells_orders
            .push_back(LiveOrder::new(101.0, 1.0, "S1".to_string()));

        let state = gen.snapshot_state();
        let json = serde_json::to_string(&state).expect("serialize");
        let restored: SymbolState = serde_json::from_str(&json).expect("deserialize");

        let mut gen2 = test_gen(StrategyConfig::default());
        gen2.restore_state(restored);
        assert_eq!(gen2.position, 3.5);
        assert_eq!(gen2.live_buys_orders.len(), 1);
        assert_eq!(gen2.live_buys_orders[0].order_id, "B1");
        assert_eq!(gen2.live_sells_orders[0].order_id, "S1");
    }

    #[tokio::test]
    async fn amend_moves_levels_without_cancelling() {
        let mut gen = test_gen(StrategyConfig::default());
        let book = test_book(&[(100.0, 10.0)], &[(100.5, 10.0)]);
        for i in 0..4 {
            gen.live_buys_orders
                .push_back(LiveOrder::new(99.9 - i as f64 * 0.1, 1.0, format!("B{}", i)));
            gen.live_sells_orders
                .push_back(LiveOrder::new(100.6 + i as f64 * 0.1, 1.0, format!("S{}", i)));
        }
        gen.last_update_price = book.mid_price;

        let moved = test_book(&[(101.0, 10.0)], &[(101.5, 10.0)]);
        gen.update_grid(empty_private(), 0.0, 0.0, moved, "BTCUSDT".to_string())
            .await;

        assert_eq!(gen.grid_amends, 1, "grid should have been amended in place");
        assert_eq!(gen.live_buys_orders.len(), 4);
        assert_eq!(gen.live_buys_orders[0].order_id, "B0", "ids must be kept");
        assert!(gen.live_buys_orders[0].price > 99.9, "levels should have moved up");
    }
}
