use std::{borrow::Cow, collections::VecDeque};

use binance::api::Binance;
use binance::futures::account::{FuturesAccount, TimeInForce};
use bybit::{
    BatchPlaceRequest, Bybit, BybitError, CancelAllRequest, Category, FastExecData, OrderRequest,
    PositionManager, PositionRequest, Side,
};
use skeleton::{
    exchanges::{
        ex_binance::BinanceClient,
        ex_bybit::BybitClient,
        exchange::{Client, Exchange, PrivateData},
    },
    util::{
        helpers::{geometric_weights, geomspace, nbsqrt, round_step, Round},
        localorderbook::LocalBook,
    },
};

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

/// The exchange-specific order management backend used by `QuoteGenerator`.
enum OrderManagement {
    Bybit(BybitClient),
    Binance(BinanceClient),
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
    pub fn new(
        client: Client,
        asset: f64,
        leverage: f64,
        orders_per_side: usize,
        final_order_distance: f64,
        rate_limit: u32,
    ) -> Self {
        let trader = match client {
            Client::Bybit(cl) => OrderManagement::Bybit(cl),
            Client::Binance(cl) => OrderManagement::Binance(cl),
        };
        QuoteGenerator {
            client: trader,
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
    /// maximum of 3.7 times the minimum.
    ///
    /// 1 bps = 0.01% = 0.0001
    fn adjusted_spread(preferred_spread: f64, book: &LocalBook) -> f64 {
        let min_spread = {
            if preferred_spread == 0.0 {
                bps_to_decimal(27.0) * book.get_mid_price()
            } else {
                bps_to_decimal(preferred_spread) * book.get_mid_price()
            }
        };
        book.get_spread().clip(min_spread, min_spread * 3.7)
    }

    /// Updates the bounds spread used to decide when the grid is out of bounds.
    fn bounds_spread(&mut self, spread: f64, last_update_price: f64, book: &LocalBook) {
        let min_spread = {
            if spread == 0.0 {
                bps_to_decimal(27.0) * last_update_price
            } else {
                bps_to_decimal(spread) * last_update_price
            }
        };
        self.bounds_spread = book.get_spread().clip(min_spread, min_spread * 3.7)
    }

    /// Generates a grid of quotes based on the current book, skew, and inventory.
    ///
    /// The grid is built around the mid price, skewed toward the side with
    /// buying pressure, and adjusted for inventory to avoid over-exposure in
    /// either direction (Avellaneda-Stoikov style inventory control).
    fn generate_quotes(&mut self, symbol: String, book: &LocalBook, skew: f64) -> Vec<BatchOrder> {
        let start = book.get_mid_price();
        let preferred_spread = self.minimum_spread;
        let curr_spread = QuoteGenerator::adjusted_spread(preferred_spread, book);
        let half_spread = curr_spread / 2.0;
        let notional = book.min_notional;

        // Correct the market skew by the current inventory to avoid building
        // up too large a position in one direction.
        let inventory_factor = nbsqrt(self.inventory_delta);
        let skew_factor = skew * (1.0 - inventory_factor.abs());
        let inventory_adjustment = -0.63 * inventory_factor;
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

        let clipped_r = aggression.clip(0.10, 0.63);

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

        let clipped_r = aggression.clip(0.10, 0.63);

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
                    sort_grid(&mut self.live_buys_orders, -1);
                    for sell_order in sells {
                        self.live_sells_orders.push_back(sell_order);
                    }
                    sort_grid(&mut self.live_sells_orders, 1);
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
    fn check_for_fills(&mut self, data: PrivateData) -> bool {
        // Binance reports cumulative filled quantities; Bybit reports increments.
        let cumulative = matches!(&data, PrivateData::Binance(_));

        let fills = match data {
            PrivateData::Bybit(data) => data.executions,
            PrivateData::Binance(data) => data.into_fastexec(),
        };

        let mut fill_occurred = false;

        for FastExecData {
            order_id,
            exec_qty,
            side,
            ..
        } in fills
        {
            // exec_qty is typed f64 in rs_bybit 0.4 (per-execution increments
            // for Bybit, cumulative fills for Binance via into_fastexec).
            let exec_qty_float = exec_qty;
            if exec_qty_float <= 0.0 {
                continue;
            }

            // Normalize side casing ("Buy"/"BUY"/"buy").
            let is_buy = side.eq_ignore_ascii_case("buy");

            let queue = if is_buy {
                &mut self.live_buys_orders
            } else {
                &mut self.live_sells_orders
            };

            for i in 0..queue.len() {
                if queue[i].order_id != order_id {
                    continue;
                }

                let remaining = (queue[i].qty - queue[i].filled_qty).max(0.0);
                let delta = if cumulative {
                    (exec_qty_float - queue[i].filled_qty).max(0.0)
                } else {
                    exec_qty_float
                }
                .min(remaining);

                if delta > 0.0 {
                    fill_occurred = true;
                    queue[i].filled_qty += delta;
                    if is_buy {
                        self.position += delta;
                    } else {
                        self.position -= delta;
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
        }
        fill_occurred
    }

    /// Determines whether the current grid is out of bounds and needs to be
    /// replaced: no live orders, the mid price moved outside the bounds, a
    /// fill occurred, or the grid is stale.
    async fn out_of_bounds(
        &mut self,
        book: &LocalBook,
        symbol: String,
        private: PrivateData,
    ) -> bool {
        let mut out_of_bounds = false;

        let bounds = self.bounds_spread;
        let current_bid_bounds = self.last_update_price - bounds;
        let current_ask_bounds = self.last_update_price + bounds;

        // Force a refresh when the grid has been untouched for 3 minutes.
        let stale = book.last_update.saturating_sub(self.time_limit) > 180_000;

        let fill_occurred = self.check_for_fills(private);
        self.inventory_delta(book);

        let bounds_check =
            book.mid_price < current_bid_bounds || book.mid_price > current_ask_bounds;

        if self.live_buys_orders.is_empty() && self.live_sells_orders.is_empty() {
            out_of_bounds = true;
            self.last_update_price = book.mid_price;
            return out_of_bounds;
        }

        if self.last_update_price != 0.0 && (bounds_check || fill_occurred || stale) {
            // Risk-reducing cancels are never skipped for rate-limit reasons;
            // the counter is only tracked for observability.
            if self.client.cancel_all(symbol.as_str()).await.is_ok() {
                out_of_bounds = true;
                // cancel-all semantics: everything is gone once this succeeds.
                self.live_buys_orders.clear();
                self.live_sells_orders.clear();
                self.last_update_price = book.mid_price;
                self.cancel_limit = self.cancel_limit.saturating_sub(1);
            } else {
                self.cancel_limit = self.cancel_limit.saturating_sub(1);
            }
        }

        out_of_bounds
    }

    /// The core strategy loop: refresh the spread bounds, reset rate limits on
    /// a one-second cadence, periodically re-sync the position with the
    /// exchange, and replace the grid when it goes out of bounds.
    pub async fn update_grid(
        &mut self,
        private: PrivateData,
        skew: f64,
        book: LocalBook,
        symbol: String,
    ) {
        self.bounds_spread(self.minimum_spread, self.last_update_price, &book);

        // Reset rate-limit counters roughly once per second.
        if self.time_limit > 1 {
            let elapsed = book.last_update.saturating_sub(self.time_limit);
            if elapsed > 1000 {
                self.rate_limit = self.initial_limit;
                self.cancel_limit = self.initial_limit;
            }
        }

        // Reconcile the local position with the exchange every 60 seconds.
        if self.last_position_sync == 0
            || book.last_update.saturating_sub(self.last_position_sync) > 60_000
        {
            self.sync_position(&symbol).await;
            self.last_position_sync = book.last_update;
        }

        if self.out_of_bounds(&book, symbol.clone(), private).await {
            self.inventory_delta(&book);

            let orders = self.generate_quotes(symbol.clone(), &book, skew);

            if self.rate_limit > 1 {
                self.send_batch_orders(orders).await;
            } else {
                eprintln!(
                    "Rate limit exhausted for {}; skipping grid placement until refresh",
                    symbol
                );
            }

            self.time_limit = book.last_update;
        }
    }
}

/// A live resting order tracked by the strategy.
#[derive(Debug, Clone)]
pub struct LiveOrder {
    pub price: f64,
    pub qty: f64,
    pub order_id: String,
    /// Quantity of this order that has already been booked into the position.
    pub filled_qty: f64,
}

impl LiveOrder {
    pub fn new(price: f64, qty: f64, order_id: String) -> Self {
        LiveOrder {
            price,
            qty,
            order_id,
            filled_qty: 0.0,
        }
    }
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
/// buys, best ask first (ascending price) for sells.
fn sort_grid(orders: &mut VecDeque<LiveOrder>, side: i32) {
    orders.make_contiguous().sort_by(|a, b| {
        if side > 0 {
            a.price
                .partial_cmp(&b.price)
                .unwrap_or(std::cmp::Ordering::Equal)
        } else {
            b.price
                .partial_cmp(&a.price)
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
                // Place orders sequentially and track each acceptance
                // individually. GTX (Good Till Crossing) is the maker-only
                // time-in-force, the Binance equivalent of post-only.
                let api: FuturesAccount =
                    Binance::new(Some(client.key.clone()), Some(client.secret.clone()));
                let mut buy_array = VecDeque::new();
                let mut sell_array = VecDeque::new();
                for BatchOrder(qty, price, symbol, side) in order_array {
                    let result = if side < 0 {
                        api.limit_sell(symbol.clone(), qty, price, TimeInForce::GTX)
                            .await
                    } else {
                        api.limit_buy(symbol.clone(), qty, price, TimeInForce::GTX)
                            .await
                    };
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
        }
    }
}
