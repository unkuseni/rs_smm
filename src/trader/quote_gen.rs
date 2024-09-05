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
        exchange::{Client, Exchange, PrivateData},
    },
    util::{
        helpers::{geometric_weights, geomspace, round_step, Round},
        localorderbook::LocalBook,
    },
};
use tokio::task;

// [qty, price, symbol, side] side is -1 for sell and 1 for buy
// The BatchOrder struct is used to represent an order that will be placed or cancelled in a batch operation.
// It contains the following fields:
// - qty: The quantity of the order.
// - price: The price of the order.
// - symbol: The symbol of the order (e.g. "BTCUSDT").
// - side: The side of the order. It can be either -1 for a sell order or 1 for a buy order.
#[derive(Debug, Clone)]
pub struct BatchOrder(f64, f64, String, i32);

// The new() method is used to create a new instance of BatchOrder.
// It takes the following parameters:
// - qty: The quantity of the order.
// - price: The price of the order.
// - side: The side of the order.
// It returns an instance of BatchOrder.
impl BatchOrder {
    pub fn new(qty: f64, price: f64, side: i32) -> Self {
        // Create a new instance of BatchOrder with the provided parameters.
        // The symbol field is initially an empty string.
        BatchOrder(qty, price, "".to_string(), side)
    }
}

/// The `OrderManagement` enum is used to represent the type of order management system
/// being used by the `QuoteGenerator`. It can be either a `Bybit` or `Binance` client.
enum OrderManagement {
    /// The `Bybit` variant represents the Bybit order management system.
    Bybit(BybitClient),
    /// The `Binance` variant represents the Binance order management system.
    Binance(BinanceClient),
}

/// The `QuoteGenerator` struct is used to generate quotes for a market making strategy.
/// It contains the following fields:
///
/// * `client` - The exchange client used to place orders. It can be either a Bybit or Binance client.
/// * `minimum_spread` - The minimum spread that the quote generator will use.
/// * `live_buys_orders` - A queue of live buy orders that have been placed.
/// * `live_sells_orders` - A queue of live sell orders that have been placed.
/// * `position` - The current position of the strategy.
/// * `max_position_usd` - The maximum position that the strategy can hold in USD.
/// * `inventory_delta` - The inventory delta of the strategy.
/// * `total_order` - The total number of orders that have been placed.
/// * `adjusted_spread` - The adjusted spread that the quote generator will use.
/// * `final_order_distance` - The final order distance that the quote generator will use.
/// * `last_update_price` - The last update price of the market.
/// * `rate_limit` - The rate limit of the exchange.
/// * `time_limit` - The time limit of the exchange.
/// * `cancel_limit` - The cancel limit of the exchange.
pub struct QuoteGenerator {
    client: OrderManagement,
    minimum_spread: f64,
    pub live_buys_orders: VecDeque<LiveOrder>,
    pub live_sells_orders: VecDeque<LiveOrder>,
    pub position: f64,
    max_position_usd: f64,
    pub inventory_delta: f64,
    total_order: usize,
    adjusted_spread: f64,
    final_order_distance: f64,
    last_update_price: f64,
    initial_limit: u32,
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
    /// * `orders_per_side` - The total number of orders to be placed on each side.
    /// * `final_order_distance` - The final order distance that the quote generator will use.
    /// * `rate_limit` - The rate limit of the exchange.
    ///
    /// # Returns
    ///
    /// A new `QuoteGenerator` instance.
    pub fn new(
        client: Client,            // The exchange client used to place orders.
        asset: f64,                // The asset value.
        leverage: f64,             // The leverage value.
        orders_per_side: usize,    // The total number of orders to be placed on each side.
        final_order_distance: f64, // The final order distance that the quote generator will use.
        rate_limit: u32,           // The rate limit of the exchange.
    ) -> Self {
        // Create the appropriate trader based on the exchange client.
        // If the client is a Bybit client, create a Bybit trader.
        // If the client is a Binance client, create a Binance trader.
        let trader = match client {
            Client::Bybit(cl) => OrderManagement::Bybit(cl),
            Client::Binance(cl) => OrderManagement::Binance(cl),
        };
        // Create a new `QuoteGenerator` instance.
        QuoteGenerator {
            // Set the client to the created trader.
            client: trader,
            // Create empty VecDeque for live buy orders with a capacity of 5.
            live_buys_orders: VecDeque::new(),
            // Create empty VecDeque for live sell orders with a capacity of 5.
            live_sells_orders: VecDeque::new(),
            // Initialize the position to 0.0.
            position: 0.0,
            // Set the inventory delta to 0.0.
            inventory_delta: 0.0,
            // Set the maximum position USD to 0.0.
            max_position_usd: QuoteGenerator::update_max(asset, leverage),
            // Set the total order to twice the number of orders per side.
            total_order: orders_per_side * 2,
            // Set the preferred spread to 0.0.
            minimum_spread: 0.0,
            // Set the adjusted spread to 0.0.
            adjusted_spread: 0.0,
            // Set the final order distance to the provided value.
            final_order_distance,

            // Initialize the last update price to 0.0.
            last_update_price: 0.0,

            // Set the intial rate limit to the provided value.
            initial_limit: rate_limit,
            // Set the rate limit to the provided value.
            rate_limit,

            // Initialize the time limit to 0.
            time_limit: 0,

            // Set the cancel limit to the provided rate limit.
            cancel_limit: rate_limit,
        }
    }

    /// Updates the maximum position USD.
    ///
    /// This function calculates the maximum amount of USD that can be allocated for the trading position.
    /// It multiplies the `asset` value by 0.95, which represents 5% of the total asset value as safety margin.
    /// The result is then assigned to the `max_position_usd` field.
    ///
    /// # Details
    ///
    /// The `asset` value represents the total value of the trading position. By multiplying it by 0.95,
    /// we leave 5% of the total asset value as a safety margin. This ensures that there is always some
    /// buffer for potential market movements and unexpected events.
    ///
    /// The `max_position_usd` field is used to determine the maximum amount of USD that can be allocated
    /// for the trading position. This value is used to calculate the maximum position quantity based on the
    /// current market conditions.
    pub fn update_max(asset: f64, leverage: f64) -> f64 {
        // Calculate the maximum position USD by multiplying the asset value by 0.95.
        let safety_margin: f64 = 0.93;
        (asset * leverage) * safety_margin
    }

    /// Sets the preferred spread for the quote generator.
    ///
    /// The preferred spread is the minimum spread that the quote generator will use when generating
    /// quotes. It is set based on the mid price in the order book.
    ///
    /// # Parameters
    ///
    /// * `spread_in_bps`: The preferred spread in basis points (bps). This is the minimum spread that
    ///                    the quote generator will use when generating quotes.
    ///
    /// # Details
    ///
    /// The preferred spread is used to determine the minimum spread that the quote generator will
    /// use when generating quotes. It is the minimum spread that the quote generator will use to
    /// ensure that the quotes it generates are profitable.
    ///
    /// The spread is set in basis points (bps) and is converted to a decimal representation before
    /// it is used to calculate the minimum spread. The minimum spread is then used to calculate the
    /// final spread that the quote generator will use.
    ///
    /// The final spread is calculated by multiplying the minimum spread by the mid price in the
    /// order book. The mid price is the average of the best ask and best bid prices in the order book.
    /// The final spread is then used to calculate the ask and bid prices for the quotes that the
    /// quote generator generates.
    pub fn set_spread(&mut self, spread_in_bps: f64) {
        // Set the minimum spread to the provided spread in basis points.
        self.minimum_spread = spread_in_bps;
    }

    /// Updates the inventory delta based on the quantity and price.
    ///
    /// This function calculates the inventory delta by dividing the position quantity by the maximum
    /// position quantity in USD. The resulting value represents the position's exposure to the market
    /// as a ratio of the maximum position quantity. The maximum position quantity is calculated by
    /// multiplying the asset value by the safety margin (95% of the total asset value).
    ///
    /// # Parameters
    ///
    /// * `mid_price`: The mid price of the asset in USD.
    ///
    /// # Details
    ///
    /// The inventory delta is a measure of the position's exposure to the market. It represents the
    /// ratio of the position's quantity to the maximum position quantity in USD. The maximum position
    /// quantity is calculated by multiplying the asset value by the safety margin, which is 95%
    /// of the total asset value. This safety margin ensures that there is a buffer for potential market
    /// movements and unexpected events.
    ///
    /// The resulting inventory delta is then assigned to the `inventory_delta` field, which is a
    /// measure of the position's exposure to the market.
    pub fn inventory_delta(&mut self, book: &LocalBook) {
        // Calculate the inventory delta by dividing the position quantity by the maximum position
        // quantity in USD.
        self.inventory_delta = (self.position * book.get_mid_price()) / self.max_position_usd;
    }

    /// Adjusts the spread by clipping it to a minimum spread and a maximum spread.
    ///
    /// This function calculates the adjusted spread by calling the `get_spread` method on the
    /// `book` parameter and clipping the result to a minimum spread and a maximum spread.
    /// 1 bps = 0.01% = 0.0001
    /// The minimum spread is calculated based on the preferred spread. If the preferred spread is 0.0,
    /// the minimum spread is 25 basis points times the mid price of the order book. Otherwise, the
    /// minimum spread is the preferred spread converted to decimal format times the mid price of the
    /// order book.
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
            // If the preferred spread is 0.0, the minimum spread is 25 basis points times the mid price of the order book.
            if preferred_spread == 0.0 {
                bps_to_decimal(25.0) * book.get_mid_price()
            }
            // Otherwise, the minimum spread is the preferred spread converted to decimal format times the mid price of the order book.
            else {
                bps_to_decimal(preferred_spread) * book.get_mid_price()
            }
        };

        // Get the spread from the order book and clip it to the minimum spread and a maximum spread of 3.7 times the minimum spread.
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
    /// This function first gets the start price from the order book.
    /// Then it calculates the preferred spread as a percentage of the start price.
    /// Next, it calculates the adjusted spread by calling the `adjusted_spread` method.
    /// After that, it calculates the half spread by dividing the spread by 2.
    /// It also gets the minimum notional from the order book.
    /// Then it generates the orders based on the skew value.
    /// If skew is positive, it calls the `positive_skew_orders` method.
    /// If skew is negative, it calls the `negative_skew_orders` method.
    /// Finally, it adds the symbol to each order.
    ///
    /// NOTES: From Cartea, 2018
    /// If imbalance is buy heavy use positive skew quotes, for sell heavy use negative skew quotes
    /// but for liquidations use the opposite, buy = negative skew & sell = positive skew meaning
    /// sell orders are easily filled in these periods and buy orders also
    fn generate_quotes(&mut self, symbol: String, book: &LocalBook, skew: f64) -> Vec<BatchOrder> {
        // Get the start price from the order book.
        let start = book.get_mid_price();

        // Calculate the preferred spread as a percentage of the start price.
        let preferred_spread = self.minimum_spread;

        // Calculate the adjusted spread by calling the `adjusted_spread` method.
        let curr_spread = QuoteGenerator::adjusted_spread(preferred_spread, book);

        // Calculate the half spread by dividing the spread by 2.
        let half_spread = curr_spread / 2.0;

        // Get the minimum notional from the order book.
        let notional = book.min_notional;
        // Get the corrected skew value.
        let corrected_skew = skew * (1.0 - self.inventory_delta.abs().sqrt());
        // Generate the orders based on the skew value.
        let mut orders = if corrected_skew >= 0.0 {
            // If skew is positive, generate positive skew orders.
            self.positive_skew_orders(
                half_spread,
                curr_spread,
                start,
                corrected_skew.abs(),
                notional,
                book,
            )
        } else {
            // If skew is negative, generate negative skew orders.
            self.negative_skew_orders(
                half_spread,
                curr_spread,
                start,
                corrected_skew.abs(),
                notional,
                book,
            )
        };

        // Add the symbol to each order.
        for v in orders.iter_mut() {
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
        let mut ask_prices = geomspace(ask_end, best_ask, self.total_order / 2);
        ask_prices.reverse();

        // Generate the bid sizes.
        let bid_sizes = if bid_prices.is_empty() {
            vec![]
        } else {
            // Calculate the maximum buy quantity.
            let max_buy_qty =
                (self.max_position_usd / 2.0) - (self.position * book.get_mid_price());
            // Calculate the size weights.
            let size_weights = geometric_weights(0.63, self.total_order / 2, true);
            // Calculate the sizes.
            let sizes: Vec<f64> = size_weights.iter().map(|w| w * max_buy_qty).collect();

            sizes
        };

        // Generate the ask sizes.
        let ask_sizes = if ask_prices.is_empty() {
            vec![]
        } else {
            // Calculate the maximum sell quantity.
            let max_sell_qty =
                (self.max_position_usd / 2.0) + (self.position * book.get_mid_price());
            // Calculate the size weights.
            let size_weights = geometric_weights(0.37, self.total_order / 2, false);
            // Calculate the sizes.
            let mut sizes: Vec<f64> = size_weights.iter().map(|w| w * max_sell_qty).collect();

            sizes.reverse();
            sizes
        };

        // Generate the batch orders.
        let mut orders = vec![];
        for (i, bid) in bid_prices.iter().enumerate() {
            // Create a new batch order with the bid size, price, and quantity.
            orders.push(BatchOrder::new(
                round_size(bid_sizes[i] / *bid, book),
                round_price(book, *bid),
                1,
            ));
            // Create a new batch order with the ask size, price, and quantity.
            orders.push(BatchOrder::new(
                round_size(ask_sizes[i] / ask_prices[i], book),
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
        let mut ask_prices = geomspace(ask_end, best_ask, self.total_order / 2);
        ask_prices.reverse();

        // Generate the bid sizes.
        let bid_sizes = if bid_prices.is_empty() {
            vec![]
        } else {
            let max_bid_qty =
                (self.max_position_usd / 2.0) - (self.position * book.get_mid_price());
            let size_weights = geometric_weights(0.37, self.total_order / 2, true);
            let sizes: Vec<f64> = size_weights.iter().map(|w| w * max_bid_qty).collect();

            sizes
        };
        // Generate the ask sizes.
        let ask_sizes = if ask_prices.is_empty() {
            vec![]
        } else {
            let max_sell_qty =
                (self.max_position_usd / 2.0) + (self.position * book.get_mid_price());
            let size_weights = geometric_weights(0.63, self.total_order / 2, false);
            let mut sizes: Vec<f64> = size_weights.iter().map(|w| w * max_sell_qty).collect();
            sizes.reverse();

            sizes
        };

        // Generate the batch orders.
        let mut orders = vec![];
        for (i, bid) in bid_prices.iter().enumerate() {
            // Create a new batch order with the bid size, price, and quantity.
            orders.push(BatchOrder::new(
                round_size(bid_sizes[i] / *bid, book),
                round_price(book, *bid),
                1,
            ));

            // Create a new batch order with the ask size, price, and quantity.
            orders.push(BatchOrder::new(
                round_size(ask_sizes[i] / ask_prices[i], book),
                round_price(book, ask_prices[i]),
                -1,
            ));
        }

        // filter orders  based on notional      // filter orders  based on notional
        orders.retain(|o| (o.0 * o.1) > notional);

        orders
    }

    /// Sends a batch of orders to the exchange asynchronously.
    ///
    /// # Arguments
    ///
    /// * `orders` - A vector of `BatchOrder` containing the orders to send.
    ///
    /// This function sends a batch of orders to the exchange asynchronously. It awaits the response
    /// from the exchange and then iterates over each response and pushes it to the appropriate
    /// queue. If the response is successful, it sorts the queues and updates the live orders. If
    /// there is an error, it prints the error message.
    async fn send_batch_orders(&mut self, orders: Vec<BatchOrder>) {
        // Send the batch orders to the exchange and await the response.
        for order in orders.chunks(10) {
            let order_response = self.client.batch_place_order(order.to_vec()).await;
            self.rate_limit -= 1;
            match order_response {
                // If the response is successful, process the orders.
                Ok(v) => {
                    // Push the orders from the first response to the live buys queue.
                    for order in v[0].clone() {
                        self.live_buys_orders.push_back(order);
                    }
                    // Sort the live buys queue and update it.
                    let sorted_buys = sort_grid(self.live_buys_orders.clone(), -1);
                    self.live_buys_orders = sorted_buys;

                    // Push the orders from the second response to the live sells queue.
                    for order in v[1].clone() {
                        self.live_sells_orders.push_back(order);
                    }
                    // Sort the live sells queue and update it.
                    let sorted_sells = sort_grid(self.live_sells_orders.clone(), 1);
                    self.live_sells_orders = sorted_sells;
                }
                // If there is an error, print the error message.
                _ => {}
            }
        }
    }

    fn check_for_fills(&mut self, data: PrivateData) {
        let fills = match data {
            PrivateData::Bybit(data) => data.executions,
            PrivateData::Binance(data) => data.into_fastexec(),
        };

        for FastExecData {
            order_id,
            exec_qty,
            side,
            ..
        } in fills
        {
            let exec_qty_str = exec_qty.replace(",", ""); // Remove commas
            if exec_qty_str.parse::<f64>().unwrap() > 0.0 {
                if side == "Buy" {
                    for (i, order) in self.live_buys_orders.clone().iter().enumerate() {
                        if order.order_id == order_id {
                            self.position += order.qty;
                            self.live_buys_orders.remove(i);
                        }
                    }
                } else {
                    for (i, order) in self.live_sells_orders.clone().iter().enumerate() {
                        if order.order_id == order_id {
                            self.position -= order.qty;
                            self.live_sells_orders.remove(i);
                        }
                    }
                }
            }
        }
    }

    async fn out_of_bounds(&mut self, book: &LocalBook, symbol: String) -> bool {
        // Initialize the `out_of_bounds` boolean to `false`.
        let mut out_of_bounds = false;
        let bounds = {
            if self.adjusted_spread > 0.0 {
                self.last_update_price * bps_to_decimal(self.adjusted_spread * 1.5)
            } else {
                self.last_update_price * bps_to_decimal(self.minimum_spread * 1.5)
            }
        };
        let (current_bid_bounds, current_ask_bounds) = (
            self.last_update_price - bounds,
            self.last_update_price + bounds,
        );

        // If there are no live orders, return `true`.
        if self.live_buys_orders.is_empty() && self.live_sells_orders.is_empty() {
            out_of_bounds = true;
            self.last_update_price = book.mid_price;
            return out_of_bounds;
        } else if self.last_update_price != 0.0 {
            // Set the `out_of_bounds` boolean to `true`.
            if self.cancel_limit > 1 {
                if book.mid_price < current_bid_bounds
                    || book.mid_price > current_ask_bounds
                    || self.live_sells_orders.len() != self.live_buys_orders.len()
                {
                    if let Ok(v) = self.client.cancel_all(symbol.as_str()).await {
                        out_of_bounds = true;
                        println!("Cancelling all orders for {}", symbol);
                        for cancelled_order in v.clone() {
                            for (i, live_order) in
                                self.live_buys_orders.clone().iter_mut().enumerate()
                            {
                                if *live_order == cancelled_order {
                                    self.live_buys_orders.remove(i);
                                }
                            }
                            for (i, live_order) in
                                self.live_sells_orders.clone().iter_mut().enumerate()
                            {
                                if *live_order == cancelled_order {
                                    self.live_sells_orders.remove(i);
                                }
                            }
                            self.last_update_price = book.mid_price;
                            self.cancel_limit -= 1;
                        }
                    } else {
                        self.cancel_limit -= 1;
                    }
                }
            }
        }
        // Return the `out_of_bounds` boolean.
        out_of_bounds
    }

    /// Updates the grid of orders with the current wallet data, skew, imbalance,
    /// order book, symbol, and price fluctuation.
    ///
    /// # Arguments
    ///
    /// * `wallet` - Private data of the wallet.
    /// * `skew` - Skew of the order book.
    /// * `book` - Current order book.
    /// * `symbol` - String representing the symbol.
    pub async fn update_grid(
        &mut self,
        private: PrivateData,
        skew: f64,
        book: LocalBook,
        symbol: String,
    ) {
        // First, update the adjusted spread by calling the `adjusted_spread` method
        // with the minimum spread and the order book.
        self.adjusted_spread = QuoteGenerator::adjusted_spread(self.minimum_spread, &book);

        // Next, update the inventory delta by calling the `inventory_delta` method.
        self.inventory_delta(&book);

        // If the time limit is greater than 1, check if the order book's last update
        // is greater than the time limit minus 1000 milliseconds.
        // If it is, update the rate limit and cancel limit to the provided rate limit.
        if self.time_limit > 1 {
            let condition = (book.last_update - self.time_limit) > 1000;
            if condition == true {
                self.rate_limit = self.initial_limit;
                self.cancel_limit = self.initial_limit;
            }
        }

        // Check if there are any fills in the private data by calling the
        // `check_for_fills` method.
        self.check_for_fills(private);
        match self.out_of_bounds(&book, symbol.clone()).await {
            true => {
                let orders = self.generate_quotes(symbol.clone(), &book, skew);

                // Check if the order book is out of bounds with the given symbol by calling
                // the `out_of_bounds` method.
                // If it is out of bounds, generate quotes for the grid based on the order book,
                // symbol, imbalance, skew, and price fluctuation.
                // If the rate limit is greater than 1, send the generated orders to the book.
                // Finally, update the time limit to the order book's last update.
                if self.rate_limit > 1 {
                    self.send_batch_orders(orders).await;
                }

                self.time_limit = book.last_update;
            }

            false => {}
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

impl PartialEq for LiveOrder {
    fn eq(&self, other: &Self) -> bool {
        self.order_id == other.order_id
    }

    fn ne(&self, other: &Self) -> bool {
        self.order_id != other.order_id
    }
}

impl PartialOrd for LiveOrder {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.price.partial_cmp(&other.price)
    }
}

fn bps_to_decimal(bps: f64) -> f64 {
    bps / 10000.0
}

fn _bps_offset(book: &LocalBook, bps: f64) -> f64 {
    book.mid_price + (book.mid_price * bps_to_decimal(bps))
}

fn _offset(book: &LocalBook, offset: f64) -> f64 {
    book.mid_price + (book.mid_price * offset)
}

fn round_price(book: &LocalBook, price: f64) -> f64 {
    let val = book.tick_size.count_decimal_places();
    price.round_to(val as u8)
}

fn round_size(qty: f64, book: &LocalBook) -> f64 {
    round_step(qty, book.lot_size)
}

/// This function takes a `VecDeque` of `LiveOrder`s and a `side` integer as input.
/// It sorts the `VecDeque` in ascending order if the `side` is greater than 1.
/// Otherwise, it sorts the `VecDeque` in descending order.
/// It then returns a new `VecDeque` with the sorted orders.
fn sort_grid(orders: VecDeque<LiveOrder>, side: i32) -> VecDeque<LiveOrder> {
    // Create a new `Vec` by consuming the `VecDeque`
    let mut vec = Vec::from(orders);

    // Sort the `Vec` either in ascending or descending order based on the `side`
    if side > 0 {
        vec.sort_by(|a, b| a.partial_cmp(b).unwrap()); // Sort the Vec in ascending order
    } else {
        vec.sort_by(|a, b| b.partial_cmp(a).unwrap()); // Sort the Vec in descending order
    }

    // Create a new `VecDeque` by consuming the sorted `Vec`
    let sorted_vecdeque: VecDeque<LiveOrder> = VecDeque::from(vec);

    // Return the sorted `VecDeque`
    sorted_vecdeque
}

impl OrderManagement {
    async fn place_buy_limit(&self, qty: f64, price: f64, symbol: &str) -> Result<LiveOrder, ()> {
        match self {
            OrderManagement::Bybit(trader) => {
                let client = trader.trader();
                if let Ok(v) = client
                    .place_futures_limit_order(
                        bybit::model::Category::Linear,
                        symbol,
                        Side::Buy,
                        qty,
                        price,
                        0,
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
                    if let Ok(v) = client.trader().limit_buy(
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
                let client = trader.trader();
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
                    if let Ok(v) = client.trader().limit_sell(
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
                let client = trader.trader();
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
                    if let Ok(v) = client.trader().market_buy(symbol, qty) {
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
                let client = trader.trader();
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
                    if let Ok(v) = client.trader().market_sell(symbol, qty) {
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
                let client = trader.trader();
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
                        .trader()
                        .cancel_order(symbol.clone(), order.order_id.parse::<u64>().unwrap())
                    {
                        if let Ok(v) = client.trader().limit_sell(
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
                let client = trader.trader();
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
                        .trader()
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
                let client = trader.trader();
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
                    if let Ok(_) = client.trader().cancel_all_open_orders(symbol) {
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
                let client = trader.trader();
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

    /// Asynchronously places a batch of orders for a given symbol and returns a vector of queues
    /// containing the live orders.
    ///
    /// # Arguments
    ///
    /// * `order_array` - A vector of `BatchOrder` structs representing the orders to be placed.
    ///
    /// # Returns
    ///
    /// * `Result<Vec<VecDeque<LiveOrder>>, ()>` - A vector of queues containing the live orders,
    /// or an error if the batch placement fails.
    async fn batch_place_order(
        &self,
        order_array: Vec<BatchOrder>,
    ) -> Result<Vec<VecDeque<LiveOrder>>, ()> {
        // Clone the order array for later use
        let order_array_clone = order_array.clone();

        // Initialize tracking variables for sell orders
        let mut tracking_sells = vec![];
        let mut index = 0;

        // Create the order requests for Bybit
        let order_arr = {
            let mut arr = vec![];
            for BatchOrder(qty, price, symbol, side) in order_array_clone {
                arr.push(OrderRequest {
                    category: bybit::model::Category::Linear,
                    symbol: Cow::Owned(symbol),
                    order_type: bybit::model::OrderType::Limit,
                    side: {
                        if side < 0 {
                            tracking_sells.push(index);
                            index += 1;
                            bybit::model::Side::Sell
                        } else {
                            index += 1;
                            bybit::model::Side::Buy
                        }
                    },
                    qty,
                    price: Some(price),
                    time_in_force: Some(Cow::Borrowed("PostOnly")),
                    ..Default::default()
                });
            }
            arr
        };

        // Place the orders with Bybit
        match self {
            OrderManagement::Bybit(trader) => {
                let client = trader.trader();
                let od_clone = order_array.clone();
                let req = BatchPlaceRequest {
                    category: bybit::model::Category::Linear,
                    requests: order_arr,
                };
                if let Ok(v) = client.batch_place_order(req).await {
                    let mut arr = vec![];
                    let mut buy_array = VecDeque::new();
                    let mut sell_array = VecDeque::new();
                    for (i, d) in v.result.list.iter().enumerate() {
                        for pos in tracking_sells.clone() {
                            if i == pos {
                                sell_array.push_back(LiveOrder::new(
                                    od_clone[i].1.clone(),
                                    od_clone[i].0.clone(),
                                    d.order_id.to_string(),
                                ));
                            } else {
                                buy_array.push_back(LiveOrder::new(
                                    od_clone[i].1.clone(),
                                    od_clone[i].0.clone(),
                                    d.order_id.to_string(),
                                ));
                            }
                        }
                    }
                    arr.push(buy_array);
                    arr.push(sell_array);
                    Ok(arr)
                } else {
                    Err(())
                }
            }
            OrderManagement::Binance(trader) => {
                // Place the orders with Binance
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
                        .trader()
                        .custom_batch_orders(order_array.len().try_into().unwrap(), order_requests)
                    {
                        // TODO: Implement live order tracking for Binance
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
                let client = trader.trader();
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
