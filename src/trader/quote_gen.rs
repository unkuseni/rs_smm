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
        helpers::{geometric_weights, geomspace, nbsqrt, round_step, Round},
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
    minimum_spread: f64,
    pub live_buys_orders: VecDeque<LiveOrder>,
    pub live_sells_orders: VecDeque<LiveOrder>,
    pub position: f64,
    max_position_usd: f64,
    max_position_qty: f64,
    pub inventory_delta: f64,
    total_order: usize,
    final_order_distance: f64,
    rebalance_ratio: f64,
    last_update_price: f64,
    rate_limit: u32,
    time_limit: u64,
    cancel_limit: u32,
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
    pub fn new(
        client: ExchangeClient,
        asset: f64,
        leverage: f64,
        orders_per_side: usize,
        final_order_distance: f64,
        rebalance_ratio: f64,
        rate_limit: u32,
    ) -> Self {
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
            // Position
            position: 0.0,
            // Set the inventory delta to 0.0.
            inventory_delta: 0.0,
            // Set the maximum position USD to 0.0.
            max_position_usd: 0.0,
            // Set the maximum position quantity to 0.0.
            max_position_qty: 0.0,
            // Set the total order to 10.
            total_order: orders_per_side * 2,
            // Set the preferred spread to the provided value.
            minimum_spread: 0.0,
            // final order distance
            final_order_distance,
            // rebalance ratio
            rebalance_ratio,

            last_update_price: 0.0,

            rate_limit,
            time_limit: 0,
            cancel_limit: rate_limit,
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
        self.minimum_spread = spread_in_bps;
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
    pub fn inventory_delta(&mut self) {
        // Calculate the inventory delta by dividing the price multiplied by the quantity by the
        // maximum position USD.
        self.inventory_delta = self.position / self.max_position_usd;
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
                bps_to_decimal(25.0)
            } else {
                bps_to_decimal(preferred_spread)
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
    /// * `price_flu`: The price fluctuation.
    ///
    /// # Returns
    ///
    /// A vector of `BatchOrder` objects representing the generated quotes.
    ///
    /// This function generates quotes based on the given parameters. It calculates the adjusted
    /// spread, half spread, aggression, and generates the orders based on the skew value. It
    /// also adds the symbol to each order.
    ///
    /// NOTES: From Cartea, 2018
    /// If imbalance is buy heavy use positive skew quotes, for sell heavy use negative skew quotes
    /// but for liquidations use the opposite, buy = negative skew & sell = positive skew meaning
    /// sell orders are easily filled in these periods and buy orders also
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

        // Calculate the preferred spread as a percentage of the start price.
        let preferred_spread = self.minimum_spread;

        // Calculate the adjusted spread by calling the `adjusted_spread` method.
        let curr_spread = QuoteGenerator::adjusted_spread(preferred_spread, book) * start;

        // Calculate the half spread by dividing the spread by 2.
        let half_spread = curr_spread / 2.0;

        // Calculate the aggression based on the imbalance and skew values.
        let aggression = {
            let mut d = 0.0;
            // If the imbalance is large or not zero, set the aggression factor to 0.6.
            // Otherwise, set it to 0.1.
            if imbalance >= 0.6 || imbalance <= -0.6 {
                d += 0.6;
            } else if imbalance != 0.0 {
                d += 0.23;
            } else {
                d += 0.1;
            }
            // Multiply the aggression factor by the square root of the skew value.
            d * nbsqrt(skew)
        };

        let volatility = bps_to_decimal(price_flu) * start;
        let notional = book.min_notional;

        // Generate the orders based on the skew value.
        let mut orders = if skew >= 0.0 {
            // If the imbalance is large and the price fluctuation is negative, generate negative
            // skew orders. Otherwise, generate positive skew orders.
            if imbalance > 0.7 && volatility <= -curr_spread * 2.0 {
                self.negative_skew_orders(
                    half_spread,
                    curr_spread,
                    skew,
                    start,
                    aggression,
                    notional,
                    book,
                )
            } else {
                self.positive_skew_orders(
                    half_spread,
                    curr_spread,
                    skew,
                    start,
                    aggression,
                    notional,
                    book,
                )
            }
        } else {
            // If the imbalance is large and the price fluctuation is negative, generate positive
            // skew orders. Otherwise, generate negative skew orders.
            if imbalance > 0.7 && volatility <= -curr_spread * 2.0 {
                self.positive_skew_orders(
                    half_spread,
                    curr_spread,
                    skew,
                    start,
                    aggression,
                    notional,
                    book,
                )
            } else {
                self.negative_skew_orders(
                    half_spread,
                    curr_spread,
                    skew,
                    start,
                    aggression,
                    notional,
                    book,
                )
            }
        };

        // Add the symbol to each order.
        for v in orders.iter_mut() {
            v.2 = symbol.clone();
        }

        orders
    }

    /// Rebalances the inventory based on the given parameters.
    ///
    /// # Arguments
    ///
    /// * `symbol` - The symbol of the quote.
    /// * `book` - The order book to get the mid price from.
    /// * `rebalance_threshold` - The amount of inventory to rebalance. -1 to 1 eg 0.6 for 60% sell orders or 60% buy orders
    ///
    /// # Returns
    ///
    /// The market response from placing the order.
    ///
    /// # Description
    ///
    /// This function rebalances the inventory by placing buy or sell orders based on the calculated values.
    /// It iterates over a loop until the inventory is rebalanced or the maximum number of retries is reached.
    /// If the inventory delta is greater than the rebalance threshold, it places a sell order.
    /// If the inventory delta is less than the rebalance threshold, it places a buy order.
    /// It calculates the price and quantity for the order based on the inventory delta and the maximum position USD.
    /// It rounds the size and price to the nearest multiple of the tick size.
    /// It awaits the response from placing the order and handles the success or error cases accordingly.
    async fn rebalance_inventory(&mut self, symbol: String, book: &LocalBook) -> LiveOrder {
        // Get the start price from the order book.
        let start = book.get_mid_price();

        // Calculate the preferred spread as a percentage of the start price.
        let preferred_spread = bps_to_decimal(3.0) * start;
        // Calculate the adjusted spread by calling the `adjusted_spread` method.
        let curr_spread = preferred_spread;

        let mut market_response = LiveOrder::new(0.0, 0.0, "".to_string());

        // Place a sell order if the inventory delta is greater than the rebalance threshold.
        if self.inventory_delta > self.rebalance_ratio {
            let price = start + curr_spread;
            let qty = ((self.inventory_delta - self.rebalance_ratio).abs() * self.max_position_usd)
                / price;

            if (qty * price) > book.min_notional {
                let order_response = self
                    .client
                    // Round the size and price to the nearest multiple of the tick size.
                    .place_sell_limit(round_size(qty, book), round_price(book, price), &symbol)
                    .await;
                match order_response {
                    Ok(v) => {
                        // Return the market response.
                        market_response = v;
                        self.position -= qty * price;
                        self.inventory_delta = 0.55;
                    }
                    Err(e) => {
                        // Print the error if there is an error placing the order.
                        println!("Error placing sell order: {:?}", e);
                    }
                }
            }
        }
        // Place a buy order if the inventory delta is less than the rebalance threshold.
        else if self.inventory_delta < -self.rebalance_ratio {
            let price = start - curr_spread;
            let qty = ((self.inventory_delta - self.rebalance_ratio).abs() * self.max_position_usd)
                / price;
            if (qty * price) > book.min_notional {
                let order_response = self
                    .client
                    // Round the size and price to the nearest multiple of the tick size.
                    .place_buy_limit(round_size(qty, book), round_price(book, price), &symbol)
                    .await;
                match order_response {
                    Ok(v) => {
                        // Return the market response.
                        market_response = v;
                        self.position += qty * price;
                        self.inventory_delta = -0.55;
                    }
                    Err(e) => {
                        // Print the error if there is an error placing the order.
                        println!("Error placing buy order: {:?}", e);
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
        notional: f64,
        book: &LocalBook,
    ) -> Vec<BatchOrder> {
        // Calculate the best bid and ask prices.
        let best_bid = start - (half_spread * (1.0 - aggression));
        let best_ask = best_bid + curr_spread;

        // Calculate the end prices for bid and ask prices.
        let end = curr_spread * self.final_order_distance;
        let bid_end = best_bid - end;
        let ask_end = best_ask + end;

        // Generate the bid and ask prices.
        let bid_prices = geomspace(best_bid, bid_end, self.total_order / 2);
        let ask_prices = geomspace(best_ask, ask_end, self.total_order / 2);

        // Calculate the clipped ratio.
        let clipped_ratio = 0.5 + skew.clip(0.02, 0.48);

        // Generate the bid sizes.
        let bid_sizes = {
            // Calculate the maximum buy quantity.
            let max_buy_qty = (self.max_position_usd / 2.0) - self.position;
            // Calculate the size weights.
            let size_weights = geometric_weights(clipped_ratio, self.total_order / 2);
            // Calculate the sizes.
            let sizes: Vec<f64> = size_weights.iter().map(|w| w * max_buy_qty).collect();

            let mut size_arr = vec![];
            // Calculate the sizes divided by the prices.
            for (price, v) in bid_prices.iter().zip(sizes.iter()) {
                size_arr.push(*v / price);
            }
            size_arr
        };

        // Generate the ask sizes.
        let ask_sizes = {
            // Calculate the maximum sell quantity.
            let max_sell_qty = (self.max_position_usd / 2.0) + self.position;
            // Calculate the size weights.
            let size_weights =
                geometric_weights(clipped_ratio.powf(2.0 + aggression), self.total_order / 2);
            // Calculate the sizes.
            let sizes: Vec<f64> = size_weights.iter().map(|w| w * max_sell_qty).collect();

            let mut size_arr = vec![];
            // Calculate the sizes divided by the prices.
            for (price, v) in ask_prices.iter().zip(sizes.iter()) {
                size_arr.push(*v / *price);
            }
            size_arr
        };

        // Generate the batch orders.
        let mut orders = vec![];
        for (i, bid) in bid_prices.iter().enumerate() {
            // Create a new batch order with the bid size, price, and quantity.
            orders.push(BatchOrder::new(
                round_size(bid_sizes[i], book),
                round_price(book, *bid),
                1,
            ));
            // Create a new batch order with the ask size, price, and quantity.
            orders.push(BatchOrder::new(
                round_size(ask_sizes[i], book),
                round_price(book, ask_prices[i]),
                -1,
            ));
        }

        // filter orders  based on notional
        orders.retain(|o| (o.0 * o.1) > notional);

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
        notional: f64,
        book: &LocalBook,
    ) -> Vec<BatchOrder> {
        // Calculate the best bid and ask prices.
        let best_ask = start + (half_spread * (1.0 - aggression));
        let best_bid = best_ask - curr_spread;

        // Calculate the end prices for bid and ask prices.
        let end = curr_spread * self.final_order_distance;
        let bid_end = best_bid - end;
        let ask_end = best_ask + end;

        // Generate the bid and ask prices.
        let bid_prices = geomspace(best_bid, bid_end, self.total_order / 2);
        let ask_prices = geomspace(best_ask, ask_end, self.total_order / 2);

        // Calculate the clipped ratio.
        let clipped_ratio = 0.5 + skew.abs().clip(0.02, 0.48);

        // Generate the bid sizes.
        let bid_sizes = {
            let max_bid_qty = (self.max_position_usd / 2.0) - self.position;
            let size_weights =
                geometric_weights(clipped_ratio.powf(2.0 + aggression), self.total_order / 2);
            let sizes: Vec<f64> = size_weights.iter().map(|w| w * max_bid_qty).collect();

            let mut size_arr = vec![];
            for (price, v) in bid_prices.iter().zip(sizes.iter()) {
                size_arr.push(*v / price);
            }
            size_arr
        };

        // Generate the ask sizes.
        let ask_sizes = {
            let max_sell_qty = (self.max_position_usd / 2.0) + self.position;
            let size_weights = geometric_weights(clipped_ratio, self.total_order / 2);
            let sizes: Vec<f64> = size_weights.iter().map(|w| w * max_sell_qty).collect();
            let mut size_arr = vec![];
            for (price, v) in ask_prices.iter().zip(sizes.iter()) {
                size_arr.push(*v / *price);
            }
            size_arr
        };

        // Generate the batch orders.
        let mut orders = vec![];
        for (i, ask) in ask_prices.iter().enumerate() {
            // Place bid orders.
            orders.push(BatchOrder::new(
                round_size(bid_sizes[i], book),
                round_price(book, bid_prices[i]),
                1,
            ));
            // Place ask orders.
            orders.push(BatchOrder::new(ask_sizes[i], *ask, -1));
        }

        // filter orders  based on notional      // filter orders  based on notional
        orders.retain(|o| (o.0 * o.1) > notional);

        orders
    }

    /// Update the orders based on the given batch orders and the local order book.
    ///
    /// # Arguments
    ///
    /// * `orders` - A vector of batch orders to update.
    /// * `book` - The local order book.
    ///
    /// This function updates the orders based on the given batch orders and the local order book.
    /// If the inventory delta is greater than 0.55, remove half of the orders especially buys.
    /// If the inventory delta is less than -0.55, remove half of the orders especially sells.
    /// If the inventory delta is between -0.55 and 0.55, do nothing.
    ///
    /// If there are more than 5 batch orders, place them as a batch order.
    /// If the batch order is successful, iterate over each response and push it to the
    /// appropriate queue. If the inventory delta is greater than 0.55, push the response to
    /// `live_sells_orders`. If the inventory delta is less than -0.55, push the response to
    /// `live_buys_orders`. Otherwise, push the response to `live_sells_orders`.
    ///
    /// # Returns
    ///
    /// None
    async fn send_batch_orders(&mut self, orders: Vec<BatchOrder>) {
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

    /// Checks if the order book is out of bounds and cancels all live orders if it is.
    ///
    /// # Arguments
    ///
    /// * `book` - The order book to check.
    /// * `symbol` - The symbol to cancel orders for.
    ///
    async fn out_of_bounds(&mut self, book: &LocalBook, symbol: String) -> bool {
        let mut out_of_bounds = false;
        // Calculate the bounds based on the spread and mid price.
        let fees = bps_to_decimal(self.minimum_spread + 2.0);
        let bounds = book.get_spread().clip(fees, fees * 2.0) * self.last_update_price;
        let bid_bounds = self.last_update_price - bounds;
        let ask_bounds = self.last_update_price + bounds;

        // If there are no live orders, return.
        if self.live_buys_orders.len() == 0 && self.live_sells_orders.len() == 0 {
            out_of_bounds = true;
            return out_of_bounds;
        }
        if self.cancel_limit > 0 {
            // If the ask bounds are less than the mid price and the last update price is not 0.0,
            // cancel all orders for the given symbol.
            if book.mid_price > ask_bounds && self.last_update_price != 0.0 {
                if let Ok(_) = self.client.cancel_all(symbol.as_str()).await {
                    self.live_sells_orders.clear();
                    out_of_bounds = true;
                    println!("Cancelling all orders for {}", symbol);
                }
            }

            // If the bid bounds are greater than the mid price and the last update price is not 0.0,
            // cancel all orders for the given symbol.
            if book.mid_price < bid_bounds && self.last_update_price != 0.0 {
                if let Ok(_) = self.client.cancel_all(symbol.as_str()).await {
                    self.live_buys_orders.clear();
                    out_of_bounds = true;
                    println!("Cancelling all orders for {}", symbol);
                }
            }

            self.cancel_limit -= 1;
        }

        out_of_bounds
    }

    /// Checks for fills in the provided private data and updates the grid of live orders accordingly.
    ///
    /// # Arguments
    ///
    /// * `data` - The private data containing the executions.
    ///
    fn check_for_fills(&mut self, data: PrivateData) {
        // Clone the live buys and sells orders.
        let mut live_buy = self.live_buys_orders.clone();
        let mut live_sell = self.live_sells_orders.clone();

        // Get the executions from the private data.
        let fills = match data {
            PrivateData::Bybit(v) => v.executions,
            PrivateData::Binance(v) => v.into_fastexec(),
        };

        // Iterate over the executions and update the grid of live orders.
        for FastExecData {
            order_id,
            exec_qty,
            side,
            symbol,
            ..
        } in fills
        {
            // If the execution quantity is not zero, update the grid of live orders.
            if exec_qty != "0.0" {
                // If the side is "Buy", remove the filled order from live buys and update the position.
                if side == "Buy" {
                    for (i, order) in live_buy.clone().iter().enumerate() {
                        if order.order_id == order_id {
                            live_buy.remove(i);
                            self.max_position_qty -= order.qty;
                            self.position += order.price * order.qty;
                            println!("Bought {} {}", order.qty, symbol);
                        }
                    }
                // If the side is not "Buy", remove the filled order from live sells and update the position.
                } else {
                    for (i, order) in live_sell.clone().iter().enumerate() {
                        if order.order_id == order_id {
                            live_sell.remove(i);
                            self.max_position_qty += order.qty;
                            self.position -= order.price * order.qty;
                            println!("Sold {} {}", order.qty, symbol);
                        }
                    }
                }
            }
        }

        // Update the live buys and sells orders with the new values.
        self.live_buys_orders = live_buy;
        self.live_sells_orders = live_sell;
    }

    /// Updates the grid of orders with the current wallet data, skew, imbalance,
    /// order book, symbol, and price fluctuation.
    ///
    /// # Arguments
    ///
    /// * `wallet` - Private data of the wallet.
    /// * `skew` - Skew of the order book.
    /// * `imbalance` - Imbalance of the order book.
    /// * `book` - Current order book.
    /// * `symbol` - String representing the symbol.
    /// * `price_flu` - Price fluctuation of the order book.
    pub async fn update_grid(
        &mut self,
        wallet: PrivateData,
        skew: f64,
        imbalance: f64,
        book: LocalBook,
        symbol: String,
        price_flu: f64,
    ) {
        // Check for fills in the wallet data.
        self.check_for_fills(wallet);

        // Update the inventory delta.
        self.inventory_delta();

        // Check if the order book is out of bounds with the given symbol.
        let replace_orders = self.out_of_bounds(&book, symbol.clone()).await;

        // Update the maximum quantity of the order book.
        self.update_max_qty(book.mid_price);

        if replace_orders == true {
            // Generate quotes for the grid based on the order book, symbol, imbalance, skew,
            // and price fluctuation.
            let orders = self.generate_quotes(symbol.clone(), &book, imbalance, skew, price_flu);

            // Send the generated orders to the book.
            if self.rate_limit > 0 {
                // check if delta is greater than 55% of inventory
                self.rebalance_inventory(symbol.clone(), &book).await;
                self.send_batch_orders(orders.clone()).await;
            }

            self.rate_limit -= 1;

            // if self.rate_limit == 0 {
            //     for BatchOrder(qty, price, order_symbol, side) in orders.clone() {
            //         if side == -1 {
            //             let live_order = match self
            //                 .client
            //                 .place_sell_limit(
            //                     round_size(qty, &book),
            //                     round_price(&book, price),
            //                     &order_symbol,
            //                 )
            //                 .await
            //             {
            //                 Ok(v) => v,
            //                 Err(_) => continue,
            //             };
            //             self.live_sells_orders.push_back(live_order);
            //         } else {
            //             let live_order = match self
            //                 .client
            //                 .place_buy_limit(
            //                     round_size(qty, &book),
            //                     round_price(&book, price),
            //                     &order_symbol,
            //                 )
            //                 .await
            //             {
            //                 Ok(v) => v,
            //                 Err(_) => continue,
            //             };
            //             self.live_buys_orders.push_back(live_order);
            //         }
            //     }
            // }

            // Print the grid orders along with the mid price, the distance between the
            // ask and mid price, and the distance between the mid price and bid.
            // println!(
            // "Grid: {:#?} Ask distance: {:#?}  Bid distance: {:#?} ",
            // orders,
            // Calculate the distance between the ask price and the mid price.
            // ((orders[1].1 - book.mid_price) / (book.mid_price / 10000.0)).round(),
            // Calculate the distance between the mid price and the bid price.
            //     ((book.mid_price - orders[0].1) / (book.mid_price / 10000.0)).round()
            // );
            
            // Update bounds
            self.last_update_price = book.mid_price;
            
            //Updates the time limit
            self.time_limit = book.last_update;
        }

        // Update the time limit
        if self.time_limit > 1 {
            let condition = (book.last_update - self.time_limit) > 1000;
            if condition == true {
                self.rate_limit = 10;
                self.cancel_limit = 10;
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
                    if let Ok(_) = client
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
                    if let Ok(_) = client.binance_trader().cancel_all_open_orders(symbol) {
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
                    if let Ok(_) = client
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

