use std::{borrow::Cow, collections::VecDeque};

use binance::{account::OrderSide, futures::account::CustomOrderRequest};
use bybit::model::{
    AmendOrderRequest, BatchAmendRequest, BatchCancelRequest, BatchPlaceRequest,
    CancelOrderRequest, CancelallRequest, FastExecData, OrderRequest, Side,
};
use skeleton::{
    exchanges::{
        ex_binance::BinanceClient,
        ex_bybit::BybitClient,
        exchange::{ExchangeClient, PrivateData},
    },
    util::{
        helpers::{geometric_weights, geomspace, nbsqrt, round_step, spread_price_in_bps, Round},
        localorderbook::LocalBook,
    },
};
use tokio::task;

// [qty, price, symbol, side] side is -1 for sell and 1 for buy
#[derive(Debug, Clone)]
pub struct BatchOrder(f64, f64, String, i32);

impl BatchOrder {
    pub fn new(qty: f64, price: f64, side: i32) -> Self {
        BatchOrder(qty, price, "".to_string(), side)
    }
}

enum OrderManagement {
    Bybit(BybitClient),
    Binance(BinanceClient),
}
pub struct QuoteGenerator {
    asset: f64,
    client: OrderManagement,
    preferred_spread: f64,
    live_buys_orders: VecDeque<LiveOrder>,
    live_sells_orders: VecDeque<LiveOrder>,
    sell_amount: f64,
    buy_amount: f64,
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
    pub fn new(client: ExchangeClient, asset: f64, leverage: f64, orders_per_side: usize) -> Self {
        // Create the appropriate trader based on the exchange client.
        let trader = match client {
            ExchangeClient::Bybit(cl) => OrderManagement::Bybit(cl),
            ExchangeClient::Binance(cl) => OrderManagement::Binance(cl),
        };
        // Create a new `QuoteGenerator` instance.
        QuoteGenerator {
            // Set the asset value multiplied by the leverage.
            asset: asset * leverage,
            // Set the client to the created trader.
            client: trader,
            // Create empty VecDeque for live buy orders with a capacity of 5.
            live_buys_orders: VecDeque::with_capacity(orders_per_side),
            // Create empty VecDeque for live sell orders with a capacity of 5.
            live_sells_orders: VecDeque::with_capacity(orders_per_side),
            // Set the sell qty to 0.0.
            sell_amount: 0.0,
            // Set the buy qty to 0.0.
            buy_amount: 0.0,
            // Set the inventory delta to 0.0.
            inventory_delta: 0.0,
            // Set the maximum position USD to 0.0.
            max_position_usd: 0.0,
            // Set the maximum position quantity to 0.0.
            max_position_qty: 0.0,
            // Set the total order to 10.
            total_order: orders_per_side * 2,
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

    /// Updates the maximum position quantity by dividing the maximum position USD by the price.
    ///
    /// This function calculates the maximum position quantity by dividing the maximum position
    /// USD by the given price. The result is then assigned to the `max_position_qty` field.
    ///
    /// # Parameters
    ///
    /// * `price`: The price of the asset.
    fn update_max_qty(&mut self, price: f64) {
        // Calculate the maximum position quantity by dividing the maximum position USD by the
        // given price.
        // The result is then assigned to the `max_position_qty` field.
        self.max_position_qty = self.max_position_usd / price;
    }

    /// Updates the inventory delta based on the quantity and price.
    ///
    /// This function calculates the inventory delta by dividing the amount by the maximum position qty.
    /// The result is then assigned to the `inventory_delta` field.
    ///
    /// # Parameters
    ///
    /// * `mid_price`: The mid price of the asset.
    ///
    /// # Details
    ///
    /// The inventory delta is a measure of the position's exposure to the market. It is calculated
    /// by dividing the amount multiplied by the mid price by the maximum position qty. The maximum
    /// position qty is the maximum amount of qty that can be allocated for the trading position,
    /// after considering the safety margin of 5%.
    ///
    /// The result is then assigned to the `inventory_delta` field, which is a measure of the
    /// position's exposure to the market.
    pub fn inventory_delta(&mut self, mid_price: f64) {
        // Calculate the inventory delta by dividing the price multiplied by the quantity by the
        // maximum position USD.
        self.inventory_delta = self.buy_amount - self.sell_amount;
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

    /// Adjusts the spread by clipping it to a minimum spread and a maximum spread.
    ///
    /// This function calculates the adjusted spread by calling the `get_spread` method on the
    /// `book` parameter and clipping the result to a minimum spread and a maximum spread.
    /// 1 bps = 0.01% = 0.0001
    /// Calculate the preferred spread as a percentage of the start price.
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
        let min_spread = {
            if preferred_spread == 0.0 {
                spread_price_in_bps(book.get_mid_price() * 0.0025, book.get_mid_price())
            } else {
                preferred_spread
            }
        };

        // Get the spread from the order book and clip it to the minimum spread and a maximum
        // spread of 3.7 times the minimum spread.
        book.get_spread().clip(min_spread, min_spread * 3.7)
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
    ///
    ///
    /// NOTES: From Cartea  2018
    /// If imbalance is buy heavy use positive skew quotes, for sell heavy use negative skew quotes
    /// but for liquidations use the opposite, buy = negative skew & sell = positive skew meaning sell orders are easily filled in these periods and buy orders also
    fn generate_quotes(
        &mut self,
        symbol: String,
        book: &LocalBook,
        imbalance: f64,
        skew: f64,
        price_flu: f64,
    ) -> Vec<BatchOrder> {
        // Get the start price from the order book.
        let start = book.get_mid_price();

        let preferred_spread = self.preferred_spread;

        // Calculate the adjusted spread by calling the `adjusted_spread` method.
        let curr_spread =
            QuoteGenerator::adjusted_spread(preferred_spread, book) * book.get_tick_size();

        // Calculate the half spread by dividing the spread by 2.
        let half_spread = curr_spread / 2.0;

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
            if imbalance > 0.7 && price_flu <= -curr_spread * 2.0 {
                self.negative_skew_orders(half_spread, curr_spread, skew, start, aggression)
            } else {
                self.positive_skew_orders(half_spread, curr_spread, skew, start, aggression)
            }
        } else {
            self.negative_skew_orders(half_spread, curr_spread, skew, start, aggression)
        };

        // Add the symbol to each order.
        for mut v in orders.iter_mut() {
            v.2 = symbol.clone();
        }

        orders
    }

    /// rebalances the inventory based on the given parameters.
    ///
    /// # Parameters
    ///
    /// * `symbol`: The symbol of the quote.
    /// * `book`: The order book to get the mid price from.
    /// * `rebalance_threshold`: The amount of inventory to rebalance. 0 to 1 eg 0.6
    ///
    /// # Returns
    ///

    async fn rebalance_inventory(
        &mut self,
        symbol: String,
        book: &LocalBook,
        rebalance_threshold: f64,
    ) -> LiveOrder {
        // Get the start price from the order book.
        let start = book.get_mid_price();

        // Calculate the preferred spread as a percentage of the start price.
        let preferred_spread =
            spread_price_in_bps(book.get_mid_price() * 0.0002, book.get_mid_price());
        // Calculate the adjusted spread by calling the `adjusted_spread` method.
        let mut count = 0;
        let curr_spread = preferred_spread * book.get_tick_size();

        let mut market_response = LiveOrder::new(0.0, 0.0, "".to_string());
        loop {
            if count > 8 {
                break;
            }
            if (self.buy_amount / self.max_position_usd) >= rebalance_threshold {
                let price = start + curr_spread;
                let qty = self.buy_amount - (self.max_position_usd / 2.0);
                let order_response = self
                    .client
                    // Round the size and price to the nearest multiple of the tick size.
                    .place_sell_limit(round_size(qty, book), round_price(book, price), &symbol)
                    .await;
                match order_response {
                    Ok(v) => {
                        // return the market response.
                        market_response = v;
                        self.buy_amount -= qty;
                        break;
                    }
                    Err(e) => {
                        // Print the error if there is an error placing the order.
                        println!("Error placing buy order: {:?}", e);
                        count += 1;
                    }
                }
            } else if (self.sell_amount / self.max_position_usd) >= rebalance_threshold {
                let price = start - curr_spread;
                let qty = self.sell_amount - (self.max_position_usd / 2.0);
                let order_response = self
                    .client
                    // Round the size and price to the nearest multiple of the tick size.
                    .place_buy_limit(round_size(qty, book), round_price(book, price), &symbol)
                    .await;
                match order_response {
                    Ok(v) => {
                        // return the market response.
                        market_response = v;
                        self.sell_amount -= qty;
                        break;
                    }
                    Err(e) => {
                        // Print the error if there is an error placing the order.
                        println!("Error placing sell order: {:?}", e);
                        count += 1;
                    }
                }
            }
        }
        market_response
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
        let end = curr_spread * 5.0;
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

    async fn out_of_bounds(&mut self, book: &LocalBook, symbol: String) {
        let bounds = spread_price_in_bps(book.mid_price * 0.0002, book.mid_price);
        let bid_bounds = book.mid_price - (bounds * book.tick_size);
        let ask_bounds = book.mid_price + (bounds * book.tick_size);
        for v in self.live_sells_orders {
            if v.price <= ask_bounds {
                self.client.cancel_all(symbol.as_str()).await;
            }
        }
        for v in self.live_buys_orders {
            if v.price >= bid_bounds {
                self.client.cancel_all(symbol.as_str()).await;
            }
        }
    }

    fn check_for_fills(&mut self, data: PrivateData) {
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
                        } else {
                            self.max_position_qty -= order.qty;
                            self.buy_amount += order.price * order.qty;
                        }
                    }
                } else {
                    for (i, order) in live_sell.clone().iter().enumerate() {
                        if order.order_id == order_id {
                            live_sell.remove(i);
                            self.max_position_qty += order.qty;
                            self.sell_amount += order.price * order.qty;
                        }
                    }
                }
            }
        }
        self.live_buys_orders = live_buy;
        self.live_sells_orders = live_sell;
    }

    pub async fn update_grid(
        &mut self,
        wallet: PrivateData,
        skew: f64,
        imbalance: f64,
        book: LocalBook,
        symbol: String,
        price_flu_in_bps: f64,
    ) {
        self.update_max();
        self.set_spread(20.0);
        self.update_max_qty(book.mid_price);
        if self.live_buys_orders.is_empty() && self.live_sells_orders.is_empty() {
            let orders = self.generate_quotes(symbol, &book, imbalance, skew, price_flu_in_bps);
            if imbalance >= 0.6 || imbalance <= -0.6 {
                println!(
                    "Grid: {:#?} {:#?} Ask distance: {:#?} Bid distance: {:#?}",
                    orders,
                    book.mid_price,
                    (orders[1].1 - book.mid_price) / book.tick_size,
                    (book.mid_price - orders[0].1) / book.tick_size
                );
            }
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

fn round_size(qty: f64, book: &LocalBook) -> f64 {
    round_step(qty, book.lot_size)
}

impl OrderManagement {
    async fn place_buy_limit(&self, qty: f64, price: f64, symbol: &str) -> Result<LiveOrder, ()> {
        match self {
            OrderManagement::Bybit(trader) => {
                let client = trader.clone().bybit_trader();
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
                    if let Ok(v) = client.binance_trader().limit_buy(
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
                let client = trader.clone().bybit_trader();
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
                let task = tokio::task::spawn_blocking(move || {
                    if let Ok(v) = client.binance_trader().limit_sell(
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

    async fn market_buy(&self, qty: f64, symbol: &str) -> Result<LiveOrder, ()> {
        match self {
            OrderManagement::Bybit(trader) => {
                let client = trader.clone().bybit_trader();
                let req = OrderRequest {
                    category: bybit::model::Category::Linear,
                    symbol: Cow::Owned(symbol.to_string()),
                    side: Side::Buy,
                    order_type: bybit::model::OrderType::Market,
                    qty,
                    ..Default::default()
                };
                if let Ok(v) = client.place_custom_order(req).await {
                    Ok(LiveOrder::new(0.0, qty, v.result.order_id))
                } else {
                    println!("Could not place market order for {} qty", qty);
                    Err(())
                }
            }
            OrderManagement::Binance(trader) => {
                let symbol = symbol.to_owned();
                let client = trader.clone();
                let task = tokio::task::spawn_blocking(move || {
                    if let Ok(v) = client.binance_trader().market_buy(symbol, qty) {
                        Ok(LiveOrder::new(v.avg_price, qty, v.order_id.to_string()))
                    } else {
                        println!("Could not place market order for {} qty", qty);
                        Err(())
                    }
                });
                task.await.unwrap()
            }
        }
    }

    async fn market_sell(&self, qty: f64, symbol: &str) -> Result<LiveOrder, ()> {
        match self {
            OrderManagement::Bybit(trader) => {
                let client = trader.clone().bybit_trader();
                let req = OrderRequest {
                    category: bybit::model::Category::Linear,
                    symbol: Cow::Owned(symbol.to_string()),
                    side: Side::Sell,
                    order_type: bybit::model::OrderType::Market,
                    qty,
                    time_in_force: Some(Cow::Borrowed("IOC")),
                    ..Default::default()
                };
                if let Ok(v) = client.place_custom_order(req).await {
                    Ok(LiveOrder::new(0.0, qty, v.result.order_id))
                } else {
                    println!("Could not place market order for {} qty", qty);
                    Err(())
                }
            }
            OrderManagement::Binance(trader) => {
                let symbol = symbol.to_owned();
                let client = trader.clone();
                let task = tokio::task::spawn_blocking(move || {
                    if let Ok(v) = client.binance_trader().market_sell(symbol, qty) {
                        Ok(LiveOrder::new(v.avg_price, qty, v.order_id.to_string()))
                    } else {
                        println!("Could not place market order for {} qty", qty);
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
                let client = trader.clone().bybit_trader();
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
                let task = tokio::task::spawn_blocking(move || {
                    if let Ok(v) = client
                        .binance_trader()
                        .cancel_order(symbol.clone(), order.order_id.parse::<u64>().unwrap())
                    {
                        if let Ok(v) = client.binance_trader().limit_sell(
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
                let client = trader.clone().bybit_trader();
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
                    if let Ok(v) = client
                        .binance_trader()
                        .cancel_order(symbol, order.order_id.parse::<u64>().unwrap())
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
                let client = trader.clone().bybit_trader();
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
                    if let Ok(v) = client.binance_trader().cancel_all_open_orders(symbol) {
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
                let client = trader.clone().bybit_trader();
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
                let client = trader.clone().bybit_trader();
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
                        .binance_trader()
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
                let client = trader.clone().bybit_trader();
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
        let mut value = 0.0001;
        let mut book = LocalBook::new();
        book.lot_size = 0.00001;
        for _ in 0..25 {
            let bps = round_size(0.3453556, &book);
            println!("Spread decimal bps: {} {} ", bps, value);
            value += 0.0001;
        }
    }
}
