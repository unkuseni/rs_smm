use std::{borrow::Cow, collections::VecDeque};

use binance::{
    account::OrderSide,
    futures::account::{CustomOrderRequest, FuturesAccount},
};
use bybit::{
    model::{
        AmendOrderRequest, BatchAmendRequest, BatchCancelRequest, BatchPlaceRequest,
        CancelOrderRequest, CancelallRequest, FastExecData, OrderRequest, Side,
    },
    trade::Trader,
};
use skeleton::{
    exchanges::exchange::{ExchangeClient, PrivateData},
    util::{
        helpers::{geometric_weights, geomspace, nbsqrt, round_step, spread_decimal_bps, Round},
        localorderbook::LocalBook,
    },
};
use tokio::task;

type BybitTrader = Trader;
type BinanceTrader = FuturesAccount;
// [qty, price, symbol, side] side is -1 for sell and 1 for buy
#[derive(Debug, Clone)]
pub struct BatchOrder(f64, f64, String, i32);

impl BatchOrder {
    pub fn new(qty: f64, price: f64, side: i32) -> Self {
        BatchOrder(qty, price, "".to_string(), side)
    }
}

enum OrderManagement {
    Bybit(BybitTrader),
    Binance(BinanceTrader),
}
pub struct QuoteGenerator {
    asset: f64,
    client: OrderManagement,
    preferred_spread: f64,
    half_spread: f64,
    positions: f64,
    live_buys_orders: VecDeque<LiveOrder>,
    live_sells_orders: VecDeque<LiveOrder>,
    max_position_usd: f64,
    max_position_qty: f64,
    inventory_delta: f64,
    total_order: usize,
}

impl QuoteGenerator {
    /// Create a new `QuoteGenerator` instance.
    ///
    /// # Arguments
    ///
    /// * `client` - The exchange client used to place orders.
    /// * `asset` - The asset value.
    /// * `leverage` - The leverage value.
    ///
    /// # Returns
    ///
    /// A new `QuoteGenerator` instance.
    pub fn new(client: ExchangeClient, asset: f64, leverage: f64) -> Self {
        // Create the appropriate trader based on the exchange client.
        let trader = match client {
            ExchangeClient::Bybit(cl) => OrderManagement::Bybit(cl.bybit_trader()),
            ExchangeClient::Binance(cl) => OrderManagement::Binance(cl.binance_trader()),
        };
        // Create a new `QuoteGenerator` instance.
        QuoteGenerator {
            // Set the asset value multiplied by the leverage.
            asset: asset * leverage,
            // Set the rounding value to 0.
            half_spread: 0.0,
            // Set the client to the created trader.
            client: trader,
            // Set the positions to 0.0.
            positions: 0.0,
            // Create empty VecDeque for live buy orders with a capacity of 5.
            live_buys_orders: VecDeque::with_capacity(5),
            // Create empty VecDeque for live sell orders with a capacity of 5.
            live_sells_orders: VecDeque::with_capacity(5),
            // Set the inventory delta to 0.0.
            inventory_delta: 0.0,
            // Set the maximum position USD to 0.0.
            max_position_usd: 0.0,
            // Set the maximum position quantity to 0.0.
            max_position_qty: 0.0,
            // Set the total order to 10.
            total_order: 10,

            // Set the preferred spread to the provided value.
            preferred_spread: 0.0,
        }
    }

    /// Updates the maximum position USD by multiplying the asset value by 0.95.
    ///
    /// This function is used to update the maximum position USD, which is the maximum
    /// amount of USD that can be allocated for the trading position.
    pub fn update_max(&mut self) {
        // Calculate the maximum position USD by multiplying the asset value by 0.95.
        // This leaves 5% of the total asset value as safety margin.
        self.max_position_usd = self.asset * 0.95;
    }

    /// Set preferred spread based on mid price in the order book.
    pub fn set_spread(&mut self, spread_in_bps: f64) {
        self.preferred_spread = spread_in_bps;
    }

    /// Updates the inventory delta based on the quantity and price.
    ///
    /// This function calculates the inventory delta by dividing the price multiplied by the
    /// quantity by the maximum position USD. The result is then assigned to the `inventory_delta`
    /// field.
    ///
    /// # Parameters
    ///
    /// * `qty`: The quantity of the asset.
    /// * `price`: The price of the asset.
    fn inventory_delta(&mut self, qty: f64, price: f64) {
        // Calculate the inventory delta by dividing the price multiplied by the quantity by the
        // maximum position USD.
        // The result is then assigned to the `inventory_delta` field.
        self.inventory_delta = (price * qty) / self.max_position_usd;
    }

    /// Updates the maximum position quantity by dividing the maximum position USD by the price.
    ///
    /// This function calculates the maximum position quantity by dividing the maximum position
    /// USD by the given price. The result is then assigned to the `max_position_qty` field.
    ///
    /// # Parameters
    ///
    /// * `price`: The price of the asset.
    pub fn update_max_qty(&mut self, price: f64) {
        // Calculate the maximum position quantity by dividing the maximum position USD by the
        // given price.
        // The result is then assigned to the `max_position_qty` field.
        self.max_position_qty = self.max_position_usd / price;
    }

    /// Calculates the total number of live orders by adding the length of the live buy orders
    /// and live sell orders.
    ///
    /// # Returns
    ///
    /// The total number of live orders as a `usize`.
    async fn total_orders(&self) -> usize {
        let live_buys_orders_len = self.live_buys_orders.len();
        let live_sells_orders_len = self.live_sells_orders.len();

        // Add the length of the live buy orders and live sell orders to get the total number of
        // live orders.
        live_buys_orders_len + live_sells_orders_len
    }

    /// Adjusts the skew by subtracting the square root of the inventory delta.
    ///
    /// This function calculates the amount of adjustment by taking the square root of the
    /// inventory delta and subtracting it from the skew. The result is then returned as the
    /// adjusted skew.
    ///
    /// # Parameters
    ///
    /// * `skew`: The original skew value.
    ///
    /// # Returns
    ///
    /// The adjusted skew value as a `f64`.
    fn adjusted_skew(&self, mut skew: f64) -> f64 {
        // Calculate the amount of adjustment by taking the square root of the inventory delta.
        let amount = nbsqrt(self.inventory_delta);

        // Subtract the adjustment from the skew.
        skew += -amount;

        skew
    }

    /// Adjusts the spread by clipping it to a minimum spread and a maximum spread.
    ///
    /// This function calculates the adjusted spread by calling the `get_spread` method on the
    /// `book` parameter and clipping the result to a minimum spread and a maximum spread.
    ///
    /// # Parameters
    ///
    /// * `preferred_spread`: The preferred spread as a `f64`.
    /// * `book`: The order book to get the spread from.
    ///
    /// # Returns
    ///
    /// The adjusted spread as a `f64`.
    fn adjusted_spread(preferred_spread: f64, book: &LocalBook) -> f64 {
        // Calculate the minimum spread by converting the preferred spread to decimal format.
        let min_spread = bps_to_decimal(preferred_spread);

        // Get the spread from the order book and clip it to the minimum spread and a maximum
        // spread of 3.7 times the minimum spread.
        book.get_spread().clip(min_spread, min_spread * 2.0)
    }

    /// Generates quotes based on the given parameters.
    ///
    /// # Parameters
    ///
    /// * `symbol`: The symbol of the quote.
    /// * `book`: The order book to get the mid price from.
    /// * `imbalance`: The imbalance of the order book.
    /// * `skew`: The skew value.
    ///
    /// # Returns
    ///
    /// A vector of `BatchOrder` objects representing the generated quotes.
    fn generate_quotes(
        &mut self,
        symbol: String,
        book: &LocalBook,
        imbalance: f64,
        skew: f64,
    ) -> Vec<BatchOrder> {
        // Get the start price from the order book.
        let start = book.get_mid_price();

        // Calculate the preferred spread as a percentage of the start price.
        let preferred_spread = {
            if self.preferred_spread >= book.get_spread_in_bps() {
                self.preferred_spread
            } else {
                let diff = (start + (start * 0.0003)) - start;
                let count = book.tick_size.count_decimal_places() + 1;
                spread_decimal_bps(diff.round_to(count as u8))
            }
        };

        // Calculate the adjusted spread by calling the `adjusted_spread` method.
        let curr_spread = QuoteGenerator::adjusted_spread(preferred_spread, book);

        // Calculate the half spread by dividing the spread by 2.
        let half_spread = curr_spread / 2.0;
        self.half_spread = half_spread;

        // Calculate the aggression based on the imbalance and skew values.
        let aggression = {
            let mut d = 0.0;
            if imbalance > 0.6 || imbalance < -0.6 {
                d = 0.6;
            } else if imbalance != 0.0 {
                d = 0.23;
            } else {
                d = 0.1;
            }
            d * nbsqrt(skew)
        };

        // Generate the orders based on the skew value.
        let mut orders = if skew >= 0.0 {
            self.positive_skew_orders(half_spread, curr_spread, skew, start, aggression)
        } else {
            self.negative_skew_orders(half_spread, curr_spread, skew, start, aggression)
        };

        // Add the symbol to each order.
        for mut v in orders.iter_mut() {
            v.2 = symbol.clone();
        }

        orders
    }

    /// Generates a list of batch orders for positive skew.
    ///
    /// # Arguments
    ///
    /// * `half_spread` - The half spread.
    /// * `curr_spread` - The current spread.
    /// * `skew` - The skew value.
    /// * `start` - The start price.
    /// * `aggression` - The aggression value.
    ///
    /// # Returns
    ///
    /// A vector of batch orders.
    fn positive_skew_orders(
        &self,
        half_spread: f64,
        curr_spread: f64,
        skew: f64,
        start: f64,
        aggression: f64,
    ) -> Vec<BatchOrder> {
        // Calculate the best bid and ask prices.
        let best_bid = start - (half_spread * (1.0 - aggression));
        let best_ask = best_bid + curr_spread;

        // Calculate the end prices for bid and ask prices.
        let end = curr_spread * 3.7;
        let bid_end = best_bid - end;
        let ask_end = best_ask + end;

        // Generate the bid and ask prices.
        let bid_prices = geomspace(best_bid, bid_end, self.total_order / 2);
        let ask_prices = geomspace(best_ask, ask_end, self.total_order / 2);

        // Calculate the clipped ratio.
        let clipped_ratio = 0.5 + skew.clip(0.01, 0.49);

        // Generate the bid sizes.
        let bid_sizes = {
            let qty = self.max_position_qty;
            let mut size_arr = vec![];
            let arr = geometric_weights(clipped_ratio, self.total_order / 2);
            for v in arr {
                size_arr.push(qty * v);
            }
            size_arr
        };

        // Generate the ask sizes.
        let ask_sizes = {
            let qty = self.max_position_qty;
            let mut size_arr = vec![];
            let arr = geometric_weights(clipped_ratio.powf(2.0 + aggression), self.total_order / 2);
            for v in arr {
                size_arr.push(qty * v);
            }
            size_arr
        };

        // Generate the batch orders.
        let mut orders = vec![];
        for (i, bid) in bid_prices.iter().enumerate() {
            orders.push(BatchOrder::new(bid_sizes[i], *bid, 1));
            orders.push(BatchOrder::new(ask_sizes[i], ask_prices[i], -1));
        }

        orders
    }

    /// Generate a list of batch orders based on negative skew.
    ///
    /// # Arguments
    ///
    /// * `half_spread` - The half spread between best ask and best bid prices.
    /// * `curr_spread` - The current spread between best ask and best bid prices.
    /// * `skew` - The skew value.
    /// * `start` - The starting price.
    /// * `aggression` - The aggression value.
    ///
    /// # Returns
    ///
    /// A vector of batch orders.
    fn negative_skew_orders(
        &self,
        half_spread: f64,
        curr_spread: f64,
        skew: f64,
        start: f64,
        aggression: f64,
    ) -> Vec<BatchOrder> {
        // Calculate the best bid and ask prices.
        let best_ask = start + (half_spread * (1.0 - aggression));
        let best_bid = best_ask - curr_spread;

        // Calculate the end prices for bid and ask prices.
        let end = curr_spread * 3.7;
        let bid_end = best_bid - end;
        let ask_end = best_ask + end;

        // Generate the bid and ask prices.
        let bid_prices = geomspace(best_bid, bid_end, self.total_order / 2);
        let ask_prices = geomspace(best_ask, ask_end, self.total_order / 2);

        // Calculate the clipped ratio.
        let clipped_ratio = 0.5 + skew.abs().clip(0.01, 0.49);

        // Generate the bid sizes.
        let bid_sizes = {
            let qty = self.max_position_qty;
            let mut size_arr = vec![];
            let arr = geometric_weights(clipped_ratio.powf(2.0 + aggression), self.total_order / 2);
            for v in arr {
                size_arr.push(qty * v);
            }
            size_arr
        };

        // Generate the ask sizes.
        let ask_sizes = {
            let qty = self.max_position_qty;
            let mut size_arr = vec![];
            let arr = geometric_weights(clipped_ratio, self.total_order / 2);
            for v in arr {
                size_arr.push(qty * v);
            }
            size_arr
        };

        // Generate the batch orders.
        let mut orders = vec![];
        for (i, ask) in ask_prices.iter().enumerate() {
            // Place bid orders.
            orders.push(BatchOrder::new(bid_sizes[i], bid_prices[i], 1));
            // Place ask orders.
            orders.push(BatchOrder::new(ask_sizes[i], *ask, -1));
        }

        orders
    }

    /// Update the orders based on the given batch orders and the local order book.
    ///
    /// # Arguments
    ///
    /// * `orders` - A vector of batch orders to update.
    /// * `book` - The local order book.
    async fn send_orders(&mut self, orders: Vec<BatchOrder>, book: &LocalBook) {
        // If the number of batch orders is less than or equal to 5,
        // place each order individually.
        if orders.len() <= 5 {
            // Iterate over each batch order.
            for BatchOrder(qty, price, symbol, side) in orders {
                // If the order is a buy order, place a buy limit order.
                if side == 1 {
                    let order_response = self
                        .client
                        // Round the size and price to the nearest multiple of the tick size.
                        .place_buy_limit(
                            round_size(qty, book),
                            round_price(book, price),
                            symbol.as_str(),
                        )
                        .await;
                    match order_response {
                        Ok(v) => {
                            // Push the response to the live buys orders queue.
                            self.live_buys_orders.push_back(v);
                        }
                        Err(e) => {
                            // Print the error if there is an error placing the order.
                            println!("Error placing buy order: {:?}", e);
                        }
                    }
                }

                // If the order is a sell order, place a sell limit order.
                if side == -1 {
                    let order_response = self
                        .client
                        .place_sell_limit(
                            round_size(qty, book),
                            round_price(book, price),
                            symbol.as_str(),
                        )
                        .await;
                    match order_response {
                        Ok(v) => {
                            // Push the response to the live sells orders queue.
                            self.live_sells_orders.push_back(v);
                        }
                        Err(e) => {
                            // Print the error if there is an error placing the order.
                            println!("Error placing sell order: {:?}", e);
                        }
                    }
                }
            }
        } else {
            // If there are more than 5 batch orders, place them as a batch order.
            let order_response = self.client.batch_place_order(orders).await;
            match order_response {
                Ok(v) => {
                    // Iterate over each response and push it to the appropriate queue.
                    for (i, res) in v.iter().enumerate() {
                        if i == 0 || i % 2 == 0 {
                            self.live_buys_orders.push_back(res.clone());
                        } else {
                            self.live_sells_orders.push_back(res.clone());
                        }
                    }
                }
                Err(e) => {
                    // Print the error if there is an error placing the batch order.
                    println!("Error placing batch order: {:?}", e);
                }
            }
        }
    }

    pub fn check_fills(&mut self, data: PrivateData) {
        let mut live_buy = self.live_buys_orders.clone();
        let mut live_sell = self.live_sells_orders.clone();
        let fills = match data {
            PrivateData::Bybit(v) => v.executions,
            PrivateData::Binance(v) => v.into_fastexec(),
        };
        for FastExecData {
            order_id,
            exec_qty,
            side,
            ..
        } in fills
        {
            if exec_qty != "0.0" {
                if side == "Buy" {
                    for (i, order) in live_buy.clone().iter().enumerate() {
                        if order.order_id == order_id {
                            live_buy.remove(i);
                        }
                    }
                } else {
                    for (i, order) in live_sell.clone().iter().enumerate() {
                        if order.order_id == order_id {
                            live_sell.remove(i);
                        }
                    }
                }
            }
        }
        self.live_buys_orders = live_buy;
        self.live_sells_orders = live_sell;
    }

    pub fn update_grid(&mut self, wallet: PrivateData, skew: f64, imbalance: f64, book: LocalBook, symbol: String) {
        if self.live_buys_orders.is_empty() && self.live_sells_orders.is_empty() {
            self.update_max();
            self.update_max_qty(book.mid_price);
            let orders = self.generate_quotes(symbol, &book, imbalance, skew);
            println!("Grid: {:#?} {:#?}", orders, book.get_bba());
        }
    }
}
#[derive(Debug, Clone)]
pub struct LiveOrder {
    pub price: f64,
    pub qty: f64,
    pub order_id: String,
}
impl LiveOrder {
    pub fn new(price: f64, qty: f64, order_id: String) -> Self {
        LiveOrder {
            price,
            qty,
            order_id,
        }
    }
}

fn bps_to_decimal(bps: f64) -> f64 {
    bps / 10000.0
}

fn bps_offset(book: &LocalBook, bps: f64) -> f64 {
    book.mid_price + (book.mid_price * bps_to_decimal(bps))
}

fn offset(book: &LocalBook, offset: f64) -> f64 {
    book.mid_price + (book.mid_price * offset)
}

fn round_price(book: &LocalBook, price: f64) -> f64 {
    let val = book.tick_size.count_decimal_places();
    price.round_to(val as u8)
}

fn round_size(price: f64, book: &LocalBook) -> f64 {
    round_step(price, book.lot_size)
}

pub fn liquidate_inventory() {}

impl OrderManagement {
    async fn place_buy_limit(&self, qty: f64, price: f64, symbol: &str) -> Result<LiveOrder, ()> {
        match self {
            OrderManagement::Bybit(trader) => {
                let client = trader.clone();

                if let Ok(v) = client
                    .place_futures_limit_order(
                        bybit::model::Category::Linear,
                        symbol,
                        Side::Buy,
                        qty,
                        price,
                        1,
                    )
                    .await
                {
                    Ok(LiveOrder::new(price, qty, v.result.order_id))
                } else {
                    Err(())
                }
            }
            OrderManagement::Binance(trader) => {
                let symbol = symbol.to_owned();
                let client = trader.clone();
                let task = task::spawn_blocking(move || {
                    if let Ok(v) = client.limit_buy(
                        symbol,
                        qty,
                        price,
                        binance::futures::account::TimeInForce::GTC,
                    ) {
                        Ok(LiveOrder::new(price, qty, v.order_id.to_string()))
                    } else {
                        Err(())
                    }
                });
                task.await.unwrap()
            }
        }
    }

    async fn place_sell_limit(&self, qty: f64, price: f64, symbol: &str) -> Result<LiveOrder, ()> {
        match self {
            OrderManagement::Bybit(trader) => {
                let client = trader.clone();
                if let Ok(v) = client
                    .place_futures_limit_order(
                        bybit::model::Category::Linear,
                        symbol,
                        Side::Sell,
                        qty,
                        price,
                        2,
                    )
                    .await
                {
                    Ok(LiveOrder::new(price, qty, v.result.order_id))
                } else {
                    Err(())
                }
            }
            OrderManagement::Binance(trader) => {
                let symbol = symbol.to_owned();
                let client = trader.clone();
                let task = task::spawn_blocking(move || {
                    if let Ok(v) = client.limit_sell(
                        symbol,
                        qty,
                        price,
                        binance::futures::account::TimeInForce::GTC,
                    ) {
                        Ok(LiveOrder::new(price, qty, v.order_id.to_string()))
                    } else {
                        Err(())
                    }
                });
                task.await.unwrap()
            }
        }
    }

    async fn amend_order(
        &self,
        order: LiveOrder,
        qty: f64,
        price: Option<f64>,
        symbol: &str,
    ) -> Result<LiveOrder, ()> {
        match self {
            OrderManagement::Bybit(trader) => {
                let client = trader.clone();
                let req = AmendOrderRequest {
                    category: bybit::model::Category::Linear,
                    order_id: Some(Cow::Borrowed(order.order_id.as_str())),
                    price,
                    qty,
                    ..Default::default()
                };
                if let Ok(v) = client.amend_order(req).await {
                    Ok(LiveOrder::new(
                        price.unwrap_or(order.price),
                        qty,
                        v.result.order_id,
                    ))
                } else {
                    Err(())
                }
            }
            OrderManagement::Binance(trader) => {
                // TODO: binance crate doesn't have an amend_order fn. so this cancels the current and places a new one then returns the new order id
                let symbol = symbol.to_owned();
                let client = trader.clone();
                let task = task::spawn_blocking(move || {
                    if let Ok(v) =
                        client.cancel_order(symbol.clone(), order.order_id.parse::<u64>().unwrap())
                    {
                        if let Ok(v) = client.limit_sell(
                            symbol,
                            qty,
                            price.unwrap(),
                            binance::futures::account::TimeInForce::GTC,
                        ) {
                            Ok(LiveOrder::new(price.unwrap(), qty, v.order_id.to_string()))
                        } else {
                            Err(())
                        }
                    } else {
                        Err(())
                    }
                });
                task.await.unwrap()
            }
        }
    }

    async fn cancel_order(&self, order: LiveOrder, symbol: &str) -> Result<LiveOrder, ()> {
        match self {
            OrderManagement::Bybit(trader) => {
                let client = trader.clone();
                let req = CancelOrderRequest {
                    category: bybit::model::Category::Linear,
                    symbol: Cow::Borrowed(symbol),
                    order_id: Some(Cow::Borrowed(order.order_id.as_str())),
                    order_filter: None,
                    order_link_id: None,
                };
                if let Ok(v) = client.cancel_order(req).await {
                    Ok(LiveOrder::new(order.price, order.qty, v.result.order_id))
                } else {
                    Err(())
                }
            }

            OrderManagement::Binance(trader) => {
                let symbol = symbol.to_owned();
                let client = trader.clone();
                let task = task::spawn_blocking(move || {
                    if let Ok(v) =
                        client.cancel_order(symbol, order.order_id.parse::<u64>().unwrap())
                    {
                        Ok(LiveOrder::new(
                            order.price,
                            order.qty,
                            v.order_id.to_string(),
                        ))
                    } else {
                        Err(())
                    }
                });
                task.await.unwrap()
            }
        }
    }

    async fn cancel_all(&self, symbol: &str) -> Result<Vec<LiveOrder>, ()> {
        let mut arr = vec![];
        match self {
            OrderManagement::Bybit(trader) => {
                let client = trader.clone();
                let req = CancelallRequest {
                    category: bybit::model::Category::Linear,
                    symbol: symbol,
                    ..Default::default()
                };
                if let Ok(v) = client.cancel_all_orders(req).await {
                    for d in v.result.list {
                        arr.push(LiveOrder::new(0.0, 0.0, d.order_id));
                    }
                    Ok(arr)
                } else {
                    Err(())
                }
            }
            OrderManagement::Binance(trader) => {
                // TODO
                let symbol = symbol.to_owned();
                let client = trader.clone();
                let task = task::spawn_blocking(move || {
                    if let Ok(v) = client.cancel_all_open_orders(symbol) {
                        Ok(arr)
                    } else {
                        Err(())
                    }
                });
                task.await.unwrap()
            }
        }
    }

    async fn batch_cancel(
        &self,
        orders: Vec<LiveOrder>,
        symbol: &str,
    ) -> Result<Vec<LiveOrder>, ()> {
        let mut arr = vec![];
        match self {
            OrderManagement::Bybit(trader) => {
                let client = trader.clone();
                let req = BatchCancelRequest {
                    category: bybit::model::Category::Linear,
                    requests: {
                        let mut li = vec![];
                        for v in orders {
                            let order_id_string = v.order_id.clone();
                            li.push(CancelOrderRequest {
                                category: bybit::model::Category::Linear,
                                symbol: Cow::Borrowed(symbol),
                                order_id: Some(Cow::Owned(order_id_string)), // Changed to Cow::Owned
                                order_filter: None,
                                order_link_id: None,
                            });
                        }
                        li
                    },
                };
                if let Ok(v) = client.batch_cancel_order(req).await {
                    for d in v.result.list {
                        arr.push(LiveOrder::new(0.0, 0.0, d.order_id));
                    }
                    Ok(arr)
                } else {
                    Err(())
                }
            }

            OrderManagement::Binance(_) => {
                // TODO:  Write batch cancel for binance
                Ok(arr)
            }
        }
    }

    async fn batch_place_order(&self, order_array: Vec<BatchOrder>) -> Result<Vec<LiveOrder>, ()> {
        let order_array_clone = order_array.clone();
        let order_arr = {
            let mut arr = vec![];
            for BatchOrder(qty, price, symbol, side) in order_array_clone {
                arr.push(OrderRequest {
                    category: bybit::model::Category::Linear,
                    symbol: Cow::Owned(symbol),
                    order_type: bybit::model::OrderType::Limit,
                    side: {
                        if side < 0 {
                            bybit::model::Side::Sell
                        } else {
                            bybit::model::Side::Buy
                        }
                    },
                    qty,
                    price: Some(price),
                    time_in_force: Some(Cow::Borrowed("GTC")),
                    ..Default::default()
                });
            }
            arr
        };
        match self {
            OrderManagement::Bybit(trader) => {
                let client = trader.clone();
                let od_clone = order_array.clone();
                let req = BatchPlaceRequest {
                    category: bybit::model::Category::Linear,
                    requests: order_arr,
                };
                if let Ok(v) = client.batch_place_order(req).await {
                    let mut arr = vec![];
                    for (i, d) in v.result.list.iter().enumerate() {
                        arr.push(LiveOrder::new(
                            od_clone[i].1.clone(),
                            od_clone[i].0.clone(),
                            d.order_id.to_string(),
                        ));
                    }
                    Ok(arr)
                } else {
                    Err(())
                }
            }
            OrderManagement::Binance(trader) => {
                // TODO: Unimplemented by the crate binance
                let client = trader.clone();
                let order_vec = order_array.clone();
                let order_requests = {
                    let mut arr = vec![];
                    for BatchOrder(qty, price, symbol, side) in order_vec {
                        arr.push(CustomOrderRequest {
                            symbol,
                            qty: Some(qty),
                            side: if side < 0 {
                                OrderSide::Sell
                            } else {
                                OrderSide::Buy
                            },
                            price: Some(price),
                            order_type: binance::futures::account::OrderType::Limit,
                            time_in_force: Some(binance::futures::account::TimeInForce::GTC),
                            position_side: None,
                            stop_price: None,
                            close_position: None,
                            activation_price: None,
                            callback_rate: None,
                            working_type: None,
                            price_protect: None,
                            reduce_only: None,
                        });
                    }
                    arr
                };
                let task = task::spawn_blocking(move || {
                    if let Ok(v) = client
                        .custom_batch_orders(order_array.len().try_into().unwrap(), order_requests)
                    {
                        let arr = vec![];
                        Ok(arr)
                    } else {
                        Err(())
                    }
                });
                task.await.unwrap()
            }
        }
    }

    async fn batch_amend(
        &self,
        orders: Vec<LiveOrder>,
        symbol: &str,
    ) -> Result<Vec<LiveOrder>, ()> {
        match self {
            OrderManagement::Bybit(trader) => {
                let client = trader.clone();
                let order_clone = orders.clone();
                let req = BatchAmendRequest {
                    category: bybit::model::Category::Linear,
                    requests: {
                        let mut arr = vec![];
                        for v in orders {
                            arr.push(AmendOrderRequest {
                                category: bybit::model::Category::Linear,
                                symbol: Cow::Borrowed(symbol),
                                order_id: Some(Cow::Owned(v.order_id)),
                                ..Default::default()
                            });
                        }
                        arr
                    },
                };
                if let Ok(v) = client.batch_amend_order(req).await {
                    let mut arr = vec![];
                    for (i, d) in v.result.list.iter().enumerate() {
                        arr.push(LiveOrder::new(
                            order_clone[i].price,
                            order_clone[i].qty,
                            d.order_id.clone().to_string(),
                        ));
                    }
                    Ok(arr)
                } else {
                    Err(())
                }
            }
            OrderManagement::Binance(_) => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spread_decimal_bps() {
        let mut value = 0.00065;
        for _ in 0..10 {
            let diff: f64 = (0.5793 + (0.5793 * value)) - 0.5793;
            let unit = 0.0001.count_decimal_places() + 1;
            let bps = spread_decimal_bps(diff.round_to(unit as u8));
            println!("Spread decimal bps: {} {} {} ", bps, diff.round(), value);
            value += 0.0001;
        }
    }
}
