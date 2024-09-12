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
        helpers::{geometric_weights, geomspace, nbsqrt, round_step, Round},
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
                bps_to_decimal(27.0) * book.get_mid_price()
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
    /// This function is responsible for generating a set of orders (quotes) based on the current market conditions
    /// and the strategy parameters. It takes into account the order book state, market skew, and the current
    /// inventory position to create a balanced set of buy and sell orders.
    ///
    /// # Parameters
    ///
    /// * `symbol`: The trading symbol (e.g., "BTCUSD") for which quotes are being generated.
    /// * `book`: A reference to the `LocalBook` struct, which contains the current order book state.
    /// * `skew`: A float value representing the current market skew. Positive values indicate a buy-heavy market,
    ///           while negative values indicate a sell-heavy market.
    ///
    /// # Returns
    ///
    /// A vector of `BatchOrder` objects representing the generated quotes. Each `BatchOrder` contains
    /// information about the order quantity, price, symbol, and side (buy or sell).
    ///
    /// # Algorithm Overview
    ///
    /// 1. Calculate the starting price (mid price) from the order book.
    /// 2. Determine the preferred spread based on the minimum spread setting.
    /// 3. Calculate an adjusted spread based on current market conditions.
    /// 4. Compute a corrected skew value that takes into account the current inventory position.
    /// 5. Generate orders using either positive or negative skew strategies based on the corrected skew.
    /// 6. Add the trading symbol to each generated order.
    ///
    /// # Notes
    ///
    /// This implementation is based on the approach described by Cartea et al. (2018). The strategy
    /// adapts to market conditions by adjusting the skew of the orders:
    /// - In a buy-heavy market (positive skew), it generates quotes with a positive skew.
    /// - In a sell-heavy market (negative skew), it generates quotes with a negative skew.
    /// - For liquidation scenarios, the opposite approach is used to facilitate order filling.
    ///
    /// The function also considers the current inventory position to avoid over-exposure in any direction.
    fn generate_quotes(&mut self, symbol: String, book: &LocalBook, skew: f64) -> Vec<BatchOrder> {
        // Get the start price (mid price) from the order book.
        let start = book.get_mid_price();

        // Use the minimum spread as the preferred spread. This could be adjusted based on market conditions.
        let preferred_spread = self.minimum_spread;

        // Calculate the adjusted spread, which may differ from the preferred spread based on market conditions.
        let curr_spread = QuoteGenerator::adjusted_spread(preferred_spread, book);

        // Calculate half of the current spread, used for positioning orders around the mid price.
        let half_spread = curr_spread / 2.0;

        // Get the minimum notional value allowed for orders from the order book.
        let notional = book.min_notional;

        // Calculate a corrected skew value that takes into account the current inventory position.
        // This helps to avoid building up too large a position in one direction.
        let corrected_skew = skew * (1.0 - nbsqrt(self.inventory_delta));

        // Generate the orders based on the corrected skew value.
        let mut orders = if corrected_skew >= 0.00 {
            // If skew is positive (buy-heavy market), generate positive skew orders.
            self.positive_skew_orders(half_spread, curr_spread, start, skew.abs(), notional, book)
        } else {
            // If skew is negative (sell-heavy market), generate negative skew orders.
            self.negative_skew_orders(half_spread, curr_spread, start, skew.abs(), notional, book)
        };

        // Add the trading symbol to each generated order.
        for order in orders.iter_mut() {
            order.2 = symbol.clone();
        }

        // Return the vector of generated orders.
        orders
    }

    /// Generates a list of batch orders for positive skew.
    ///
    /// This function creates a set of buy and sell orders optimized for a market with positive skew.
    /// Positive skew indicates a tendency towards higher prices, so the function adjusts order placement accordingly.
    ///
    /// # Arguments
    ///
    /// * `half_spread` - Half of the current bid-ask spread.
    /// * `curr_spread` - The full current bid-ask spread.
    /// * `start` - The starting price, typically the mid-market price.
    /// * `aggression` - A factor determining how aggressively to place orders (0.0 to 1.0).
    /// * `notional` - The minimum notional value for an order to be considered valid.
    /// * `book` - Reference to the current order book state.
    ///
    /// # Returns
    ///
    /// A vector of `BatchOrder` objects representing the generated orders.
    fn positive_skew_orders(
        &self,
        half_spread: f64,
        curr_spread: f64,
        start: f64,
        aggression: f64,
        notional: f64,
        book: &LocalBook,
    ) -> Vec<BatchOrder> {
        // Calculate the best bid price, adjusting for market skew
        let best_bid = start - (half_spread * (1.0 - aggression.sqrt()));
        // Calculate the best ask price based on the best bid and current spread
        let best_ask = best_bid + curr_spread;

        // Calculate the range of prices for order placement
        let end = curr_spread * self.final_order_distance;
        let bid_end = best_bid - end;
        let ask_end = best_ask + end;

        // Generate a geometric distribution of prices for bids and asks
        let bid_prices = geomspace(best_bid, bid_end, self.total_order / 2);
        let mut ask_prices = geomspace(ask_end, best_ask, self.total_order / 2);
        ask_prices.reverse(); // Reverse ask prices to match bid price order

        // Clip the aggression factor to a reasonable range
        let clipped_r = aggression.clip(0.27, 0.73);

        // Generate bid sizes based on current inventory and market conditions
        let bid_sizes = if bid_prices.is_empty() || self.inventory_delta >= 0.90 {
            // If no bid prices or inventory is too high, don't place buy orders
            vec![]
        } else {
            // Calculate the maximum buy quantity based on position limits
            let max_buy_qty =
                (self.max_position_usd / 2.0) - (self.position * book.get_mid_price());
            // Generate size weights for a geometric distribution
            let size_weights = geometric_weights(clipped_r, self.total_order / 2, true);
            // Apply weights to the maximum buy quantity
            let sizes: Vec<f64> = size_weights.iter().map(|w| w * max_buy_qty).collect();

            sizes
        };

        // Generate ask sizes based on current inventory and market conditions
        let ask_sizes = if ask_prices.is_empty() {
            vec![]
        } else {
            // Calculate the maximum sell quantity based on position limits
            let max_sell_qty =
                (self.max_position_usd / 2.0) + (self.position * book.get_mid_price());
            // Generate size weights for a geometric distribution
            let size_weights = geometric_weights(0.37, self.total_order / 2, false);
            // Apply weights to the maximum sell quantity
            let mut sizes: Vec<f64> = size_weights.iter().map(|w| w * max_sell_qty).collect();

            sizes.reverse(); // Reverse sizes to match ask price order
            sizes
        };

        // Generate the batch orders
        let mut orders = vec![];
        for (i, bid) in bid_prices.iter().enumerate() {
            // Create buy orders if bid sizes are available
            if bid_sizes.len() >= 1 {
                orders.push(BatchOrder::new(
                    round_size(bid_sizes[i] / *bid, book), // Calculate and round the order size
                    round_price(book, *bid),               // Round the bid price
                    1,                                     // Indicate a buy order
                ));
            }
            // Create sell orders
            orders.push(BatchOrder::new(
                round_size(ask_sizes[i] / ask_prices[i], book), // Calculate and round the order size
                round_price(book, ask_prices[i]),               // Round the ask price
                -1,                                             // Indicate a sell order
            ));
        }

        // Filter out orders that don't meet the minimum notional value
        orders.retain(|o| (o.0 * o.1) > notional);

        orders // Return the generated and filtered orders
    }

    /// Generates a list of batch orders for negative skew scenarios.
    ///
    /// This function creates a set of buy and sell orders optimized for a market with negative skew.
    /// Negative skew indicates a tendency towards lower prices, so the function adjusts order placement accordingly.
    ///
    /// # Arguments
    ///
    /// * `half_spread` - Half of the current bid-ask spread.
    /// * `curr_spread` - The full current bid-ask spread.
    /// * `start` - The starting price, typically the mid-market price.
    /// * `aggression` - A factor determining how aggressively to place orders (0.0 to 1.0).
    /// * `notional` - The minimum notional value for an order to be considered valid.
    /// * `book` - Reference to the current order book state.
    ///
    /// # Returns
    ///
    /// A vector of `BatchOrder` objects representing the generated orders.
    ///
    /// # Algorithm
    ///
    /// 1. Calculate best ask and bid prices, adjusting for market skew.
    /// 2. Determine the range of prices for order placement.
    /// 3. Generate geometric distributions of prices for bids and asks.
    /// 4. Calculate bid and ask sizes based on current inventory and market conditions.
    /// 5. Create batch orders for both buy and sell sides.
    /// 6. Filter out orders that don't meet the minimum notional value.
    ///
    /// # Note
    ///
    /// This function is designed to work in tandem with `positive_skew_orders` to provide a
    /// comprehensive market making strategy that adapts to different market conditions.
    fn negative_skew_orders(
        &self,
        half_spread: f64,
        curr_spread: f64,
        start: f64,
        aggression: f64,
        notional: f64,
        book: &LocalBook,
    ) -> Vec<BatchOrder> {
        // Calculate the best ask price, adjusting for market skew
        // In a negative skew scenario, we place the best ask closer to the mid price
        let best_ask = start + (half_spread * (1.0 - aggression.sqrt()));

        // Calculate the best bid price based on the best ask and current spread
        let best_bid = best_ask - curr_spread;

        // Calculate the range of prices for order placement
        // The 'end' price is determined by the current spread and final_order_distance
        let end = curr_spread * self.final_order_distance;

        // Calculate the lowest bid price and highest ask price
        let bid_end = best_bid - end;
        let ask_end = best_ask + end;

        // Generate a geometric distribution of prices for bids and asks
        // This creates a series of prices that are closer together near the best bid/ask
        // and further apart as they move away from the mid price
        let bid_prices = geomspace(best_bid, bid_end, self.total_order / 2);
        let mut ask_prices = geomspace(ask_end, best_ask, self.total_order / 2);
        ask_prices.reverse(); // Reverse ask prices to match bid price order

        // Clip the aggression factor to a reasonable range
        let clipped_r = aggression.clip(0.27, 0.73);

        // Generate bid sizes based on current inventory and market conditions
        let bid_sizes = if bid_prices.is_empty() {
            vec![] // If no bid prices, don't place any buy orders
        } else {
            // Calculate the maximum buy quantity based on position limits
            let max_bid_qty =
                (self.max_position_usd / 2.0) - (self.position * book.get_mid_price());

            // Generate size weights for a geometric distribution
            // We use a fixed factor of 0.37 for bids in negative skew scenarios
            let size_weights = geometric_weights(0.37, self.total_order / 2, true);

            // Apply weights to the maximum buy quantity
            let sizes: Vec<f64> = size_weights.iter().map(|w| w * max_bid_qty).collect();

            sizes
        };

        // Generate ask sizes based on current inventory and market conditions
        let ask_sizes = if ask_prices.is_empty() || self.inventory_delta <= -0.90 {
            vec![] // If no ask prices or inventory is too low, don't place sell orders
        } else {
            // Calculate the maximum sell quantity based on position limits
            let max_sell_qty =
                (self.max_position_usd / 2.0) + (self.position * book.get_mid_price());

            // Generate size weights for a geometric distribution
            // We use the clipped aggression factor for asks in negative skew scenarios
            let size_weights = geometric_weights(clipped_r, self.total_order / 2, false);

            // Apply weights to the maximum sell quantity
            let mut sizes: Vec<f64> = size_weights.iter().map(|w| w * max_sell_qty).collect();
            sizes.reverse(); // Reverse sizes to match ask price order

            sizes
        };

        // Generate the batch orders
        let mut orders = vec![];
        for (i, bid) in bid_prices.iter().enumerate() {
            // Create a new batch order for buying (side = 1)
            orders.push(BatchOrder::new(
                round_size(bid_sizes[i] / *bid, book), // Calculate and round the order size
                round_price(book, *bid),               // Round the bid price
                1,                                     // Indicate a buy order
            ));

            // Create a new batch order for selling (side = -1), if ask sizes are available
            if ask_sizes.len() >= 1 {
                orders.push(BatchOrder::new(
                    round_size(ask_sizes[i] / ask_prices[i], book), // Calculate and round the order size
                    round_price(book, ask_prices[i]),               // Round the ask price
                    -1,                                             // Indicate a sell order
                ));
            }
        }

        // Filter out orders that don't meet the minimum notional value
        // This ensures that all orders meet the exchange's minimum order size requirements
        orders.retain(|o| (o.0 * o.1) > notional);

        // Return the final list of orders
        orders
    }

    /// Sends a batch of orders to the exchange asynchronously.
    ///
    /// This function is responsible for sending a batch of orders to the exchange and processing
    /// the response. It handles rate limiting, error handling, and updating the internal state
    /// of live orders.
    ///
    /// # Arguments
    ///
    /// * `orders` - A vector of `BatchOrder` containing the orders to send.
    ///
    /// # Details
    ///
    /// The function performs the following steps:
    /// 1. Splits the orders into chunks of 10 to avoid overwhelming the exchange API.
    /// 2. Sends each chunk to the exchange and awaits the response.
    /// 3. Updates the rate limit counter.
    /// 4. Processes the response:
    ///    - If successful, updates the live orders queues and sorts them.
    ///    - If there's an error, logs the error message.
    ///
    /// # Rate Limiting
    ///
    /// The function decrements the `rate_limit` counter for each batch sent. This helps in
    /// adhering to the exchange's API rate limits.
    ///
    /// # Error Handling
    ///
    /// If the exchange returns an error, the function logs a "Batch order error" message.
    /// More sophisticated error handling could be implemented here in the future.
    ///
    /// # Note
    ///
    /// This function assumes that the exchange response contains two vectors: one for buy orders
    /// and one for sell orders. This structure might need to be adjusted based on the specific
    /// exchange API being used.
    async fn send_batch_orders(&mut self, orders: Vec<BatchOrder>) {
        // Iterate over the orders in chunks of 10 to avoid overwhelming the exchange API
        for order_chunk in orders.chunks(10) {
            // Send the batch of orders to the exchange and await the response
            let order_response = self.client.batch_place_order(order_chunk.to_vec()).await;

            // Decrement the rate limit counter
            self.rate_limit -= 1;

            // Process the response from the exchange
            match order_response {
                // If the response is successful, process the orders
                Ok(response) => {
                    // Process buy orders (assumed to be in the first element of the response)
                    for buy_order in response[0].clone() {
                        // Add the new buy order to the live buy orders queue
                        self.live_buys_orders.push_back(buy_order);
                    }
                    // Sort the live buy orders and update the queue
                    let sorted_buys = sort_grid(&mut self.live_buys_orders, -1);
                    self.live_buys_orders = sorted_buys;

                    // Process sell orders (assumed to be in the second element of the response)
                    for sell_order in response[1].clone() {
                        // Add the new sell order to the live sell orders queue
                        self.live_sells_orders.push_back(sell_order);
                    }
                    // Sort the live sell orders and update the queue
                    let sorted_sells = sort_grid(&mut self.live_sells_orders, 1);
                    self.live_sells_orders = sorted_sells;
                }
                // If there is an error, log the error message
                Err(_) => {
                    println!("Batch order error");
                    // TODO: Implement more sophisticated error handling and logging
                }
            }
        }
    }

    /// Checks for and processes filled orders based on private execution data.
    ///
    /// This function updates the internal state of the QuoteGenerator by processing
    /// the execution data received from the exchange. It handles both buy and sell orders,
    /// updating the position and removing filled orders from the live order lists.
    ///
    /// # Arguments
    ///
    /// * `data`: PrivateData - The private execution data from the exchange, which can be
    ///   either from Bybit or Binance.
    ///
    /// # Details
    ///
    /// The function performs the following steps:
    /// 1. Extracts the execution data based on the exchange type.
    /// 2. Iterates through each filled order in the execution data.
    /// 3. Processes each fill, updating the position and removing the filled order from
    ///    the appropriate live order list (buy or sell).
    /// 4. Logs information about each filled order.
    ///
    /// # Note
    ///
    /// This function assumes that the execution quantity is provided as a string and may
    /// contain commas, which are removed before parsing to a float.
    fn check_for_fills(&mut self, data: PrivateData) {
        // Extract the fills data based on the exchange type
        let fills = match data {
            PrivateData::Bybit(data) => data.executions,
            PrivateData::Binance(data) => data.into_fastexec(),
        };

        // Iterate through each fill in the execution data
        for FastExecData {
            order_id,
            exec_qty,
            side,
            ..
        } in fills
        {
            // Remove commas from the execution quantity string and parse it to a float
            let exec_qty_str = exec_qty.replace(",", "");
            if let Ok(exec_qty_float) = exec_qty_str.parse::<f64>() {
                if exec_qty_float > 0.0 {
                    if side == "Buy" {
                        // Process filled buy orders
                        for (i, order) in self.live_buys_orders.clone().iter().enumerate() {
                            if order.order_id == order_id {
                                // Update the position and remove the filled order
                                self.position += order.qty;
                                println!(
                                    "Buy order filled: ID {}, Qty {}, New position {}",
                                    order_id, exec_qty, self.position
                                );
                                self.live_buys_orders.remove(i);
                                break; // Exit the loop after processing the filled order
                            }
                        }
                    } else {
                        // Process filled sell orders
                        for (i, order) in self.live_sells_orders.clone().iter().enumerate() {
                            if order.order_id == order_id {
                                // Update the position and remove the filled order
                                self.position -= order.qty;
                                println!(
                                    "Sell order filled: ID {}, Qty {}, New position {}",
                                    order_id, exec_qty, self.position
                                );
                                self.live_sells_orders.remove(i);
                                break; // Exit the loop after processing the filled order
                            }
                        }
                    }
                }
            } else {
                println!("Error parsing execution quantity: {}", exec_qty);
            }
        }
    }

    /// Determines if the current orders are out of bounds and need to be updated.
    ///
    /// This function checks if the current live orders are still valid given the current market conditions.
    /// It considers the order book, recent fills, and the current spread to make this determination.
    ///
    /// # Arguments
    ///
    /// * `&mut self` - Mutable reference to the QuoteGenerator instance.
    /// * `book` - Reference to the current LocalBook (order book).
    /// * `symbol` - The trading symbol as a String.
    /// * `private` - PrivateData containing recent trade execution information.
    ///
    /// # Returns
    ///
    /// * `bool` - True if orders are out of bounds and need updating, false otherwise.
    async fn out_of_bounds(
        &mut self,
        book: &LocalBook,
        symbol: String,
        private: PrivateData,
    ) -> bool {
        // Initialize the out_of_bounds flag to false
        let mut out_of_bounds = false;

        // Calculate the bounds for determining if orders are out of range
        let bounds = {
            if self.adjusted_spread > 0.0 {
                // Use 150% of the adjusted spread if it's set
                self.last_update_price * bps_to_decimal(self.adjusted_spread * 1.5)
            } else {
                // Otherwise, use 150% of the minimum spread
                self.last_update_price * bps_to_decimal(self.minimum_spread * 1.5)
            }
        };

        // Determine the current bid and ask bounds
        let (current_bid_bounds, current_ask_bounds) = (
            // Get the price of the first sell order, or use a default if none exists
            self.live_sells_orders
                .front()
                .unwrap_or(&LiveOrder {
                    price: self.last_update_price + bounds,
                    qty: 0.0,
                    order_id: "default".to_string(),
                })
                .clone()
                .price,
            // Get the price of the first buy order, or use a default if none exists
            self.live_buys_orders
                .front()
                .unwrap_or(&LiveOrder {
                    price: self.last_update_price - bounds,
                    qty: 0.0,
                    order_id: "default".to_string(),
                })
                .clone()
                .price,
        );

        // Process any recent fills from the private execution data
        self.check_for_fills(private);

        // Check if there are no live orders
        if self.live_buys_orders.is_empty() && self.live_sells_orders.is_empty() {
            // If no live orders, set out_of_bounds to true
            out_of_bounds = true;
            // Update the last_update_price to the current mid price
            self.last_update_price = book.mid_price;
            // Return true as we need to generate new orders
            return out_of_bounds;
        } else if self.last_update_price != 0.0 {
            // Check if we have enough cancellations left in our rate limit
            if self.cancel_limit > 1 {
                // Check if the current mid price is outside our order bounds
                if book.mid_price < current_bid_bounds || book.mid_price > current_ask_bounds {
                    // Attempt to cancel all existing orders
                    if let Ok(v) = self.client.cancel_all(symbol.as_str()).await {
                        out_of_bounds = true;

                        // Process each cancelled order
                        for cancelled_order in v.clone() {
                            // Remove cancelled buy orders from our live orders
                            for (i, live_order) in
                                self.live_buys_orders.clone().iter_mut().enumerate()
                            {
                                if *live_order == cancelled_order {
                                    self.live_buys_orders.remove(i);
                                }
                            }
                            // Remove cancelled sell orders from our live orders
                            for (i, live_order) in
                                self.live_sells_orders.clone().iter_mut().enumerate()
                            {
                                if *live_order == cancelled_order {
                                    self.live_sells_orders.remove(i);
                                }
                            }
                            // Update the last update price to the current mid price
                            self.last_update_price = book.mid_price;
                            // Decrement our cancellation limit
                            self.cancel_limit -= 1;
                        }
                    } else {
                        // If cancellation failed, still decrement the cancel limit
                        self.cancel_limit -= 1;
                    }
                }
            }
        }
        // Return the final out_of_bounds status
        out_of_bounds
    }

    /// Updates the grid of orders with the current market data and trading parameters.
    ///
    /// This function is the core of the market-making strategy, responsible for adjusting
    /// the order grid based on the latest market conditions and trading parameters.
    ///
    /// # Arguments
    ///
    /// * `private` - Private data of the wallet, containing information about current positions and balances.
    /// * `skew` - A float representing the current market skew. Positive values indicate a buy-heavy market,
    ///            while negative values indicate a sell-heavy market.
    /// * `book` - The current state of the order book, containing bid and ask prices and volumes.
    /// * `symbol` - A string representing the trading symbol (e.g., "BTCUSD").
    ///
    /// # Behavior
    ///
    /// 1. Updates the adjusted spread based on current market conditions.
    /// 2. Checks and potentially resets rate limits based on the time since the last update.
    /// 3. Determines if the current orders are out of bounds (needing adjustment).
    /// 4. If out of bounds:
    ///    a. Updates the inventory delta.
    ///    b. Generates new quotes.
    ///    c. Sends the new orders to the exchange (if within rate limits).
    /// 5. Updates the time of the last grid update.
    pub async fn update_grid(
        &mut self,
        private: PrivateData,
        skew: f64,
        book: LocalBook,
        symbol: String,
    ) {
        // Update the adjusted spread based on the current minimum spread and order book
        // This accounts for current market volatility and liquidity
        self.adjusted_spread = QuoteGenerator::adjusted_spread(self.minimum_spread, &book);

        // Check if it's time to reset the rate limits
        // This helps to manage API call frequency and avoid hitting exchange limits
        if self.time_limit > 1 {
            let condition = (book.last_update - self.time_limit) > 1000;
            if condition {
                // Reset rate limits to their initial values
                self.rate_limit = self.initial_limit;
                self.cancel_limit = self.initial_limit;
            }
        }

        // Check if the current orders are out of bounds and need adjustment
        match self.out_of_bounds(&book, symbol.clone(), private).await {
            true => {
                // Orders are out of bounds, need to adjust the grid

                // Update the inventory delta to account for any recent trades
                self.inventory_delta(&book);

                // Generate new quotes based on current market conditions
                let orders = self.generate_quotes(symbol.clone(), &book, skew);

                // Send the new orders to the exchange if within rate limits
                if self.rate_limit > 1 {
                    self.send_batch_orders(orders).await;
                }

                // Update the time of the last grid update
                self.time_limit = book.last_update;
            }

            false => {
                // Orders are still within acceptable bounds, no action needed
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
fn sort_grid(orders: &mut VecDeque<LiveOrder>, side: i32) -> VecDeque<LiveOrder> {
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
    orders.clone()
}

impl OrderManagement {
    #[allow(dead_code)]
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

    #[allow(dead_code)]
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

    #[allow(dead_code)]
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

    #[allow(dead_code)]
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

    #[allow(dead_code)]
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

    #[allow(dead_code)]
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

    #[allow(dead_code)]
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

    #[allow(dead_code)]
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
