use bybit::model::WsTrade;
use skeleton::exchanges::exchange::{Client, Exchange, PrivateData};
use skeleton::util::localorderbook::LocalBook;
use skeleton::{exchanges::exchange::MarketMessage, ss::SharedState};
use std::collections::{HashMap, VecDeque};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::interval;

use crate::features::engine::Engine;
use crate::parameters::parameters::watch;
use crate::trader::quote_gen::QuoteGenerator;

pub struct MarketMaker {
    pub features: HashMap<String, Engine>,
    pub old_books: HashMap<String, LocalBook>,
    pub old_trades: HashMap<String, VecDeque<WsTrade>>,
    pub curr_trades: HashMap<String, VecDeque<WsTrade>>,
    pub prev_avg_trade_price: HashMap<String, f64>,
    pub generators: HashMap<String, QuoteGenerator>,
    pub depths: Vec<usize>,
    pub tick_window: usize,
}

impl MarketMaker {
    /// Constructs a new `MarketMaker` instance.
    ///
    /// # Arguments
    ///
    /// * `ss` - The shared state containing information about the markets.
    /// * `assets` - The assets and their corresponding leverage.
    /// * `leverage` - The leverage for each asset.
    /// * `orders_per_side` - The number of orders to place on each side of the order book.
    /// * `final_order_distance` - The distance of the final order from the mid price.
    /// * `depths` - The depths at which to calculate imbalance ratios it uses vec![5, 50].
    ///
    /// # Returns
    ///
    /// A new `MarketMaker` instance.
    pub async fn new(
        ss: SharedState,
        assets: HashMap<String, f64>,
        leverage: f64,
        orders_per_side: usize,
        final_order_distance: f64,
        depths: Vec<usize>,
        rate_limit: u32,
        tick_window: usize,
    ) -> Self {
        // Construct the `MarketMaker` instance with the provided arguments.
        MarketMaker {
            // Initialize the `features` field with the features for each symbol.
            features: MarketMaker::build_features(ss.symbols.clone(), tick_window),
            // Initialize the `old_books` field with an empty hashmap.
            old_books: HashMap::new(),
            // Initialize the `old_trades` field with an empty hashmap.
            old_trades: HashMap::new(),
            // Initialize the `curr_trades` field with an empty hashmap.
            curr_trades: HashMap::new(),
            // Initialize the `prev_avg_trade_price` field with an empty hashmap.
            prev_avg_trade_price: HashMap::new(),
            // Initialize the `generators` field with quote generators for each symbol.
            generators: MarketMaker::build_generators(
                ss.clients,
                assets,
                orders_per_side,
                leverage,
                final_order_distance,
                rate_limit,
            )
            .await,
            // Initialize the `depths` field with the provided depths.
            depths,
            tick_window,
        }
    }

    /// Starts a loop that continuously receives and processes shared state updates.
    ///
    /// # Arguments
    ///
    /// * `receiver` - An unbounded receiver for receiving `SharedState` updates.
    ///
    /// # Returns
    ///
    /// This function does not return any value.
    pub async fn start_loop(&mut self, mut receiver: UnboundedReceiver<SharedState>) {
        let mut send = 0;
        let mut wait = interval(Duration::from_millis(600));
        // Continuously receive and process shared state updates.
        while let Some(data) = receiver.recv().await {
            // Match the exchange in the received data.
            match data.exchange.as_str() {
                "bybit" | "binance" => {
                    // Update features with the first market data in the received data.
                    self.update_features(data.markets[0].clone(), self.depths.clone());

                    // Update the strategy with the new market data and private data.
                    if send > self.tick_window {
                        self.potentially_update(data.private, data.markets[0].clone())
                            .await;
                    } else {
                        wait.tick().await;
                        send += 1;
                    }
                }
                "both" => {}
                _ => {
                    // Panic if the exchange does not match any of the specified options.
                    panic!("Invalid exchange");
                }
            }
        }
    }

    /// Builds features for each symbol in the received data.
    ///
    /// # Arguments
    ///
    /// * `symbol` - A vector of symbol names.
    ///
    /// # Returns
    ///
    /// A `HashMap` containing the symbol names as keys and `Engine` instances as values.
    fn build_features(symbol: Vec<String>, tick_window: usize) -> HashMap<String, Engine> {
        // Create a new HashMap to store the features.
        let mut hash: HashMap<String, Engine> = HashMap::new();

        // Iterate over each symbol and insert a new `Engine` instance into the HashMap.
        for v in symbol {
            // Convert the symbol name to a string and insert it into the HashMap.
            hash.insert(v, Engine::new(tick_window));
        }

        // Return the populated HashMap.
        hash
    }

    /// Builds generators for each symbol in the received data.
    ///
    /// # Arguments
    ///
    /// * `clients` - A `HashMap` containing the symbol names as keys and `ExchangeClient` instances as values.
    /// * `assets` - A `HashMap` containing the symbol names as keys and asset values as floats.
    /// * `orders_per_side` - The number of orders to place on each side of the order book.
    /// * `leverage` - The leverage to use for trading.
    /// * `final_order_distance` - The distance between the final order and the mid price.
    ///
    /// # Returns
    ///
    /// A `HashMap` containing the symbol names as keys and `QuoteGenerator` instances as values.
    async fn build_generators(
        clients: HashMap<String, Client>,
        assets: HashMap<String, f64>,
        orders_per_side: usize,
        leverage: f64,
        final_order_distance: f64,
        rate_limit: u32,
    ) -> HashMap<String, QuoteGenerator> {
        // Create a new HashMap to store the generators.
        let mut hash: HashMap<String, QuoteGenerator> = HashMap::new();

        // Iterate over each client and insert a new `QuoteGenerator` instance into the HashMap.
        for (k, v) in clients {
            // Get the asset value for the current symbol.
            let asset = assets.get(&k).unwrap().clone();

            match v.clone() {
                Client::Bybit(cl) => match cl.set_leverage(&k, leverage as u16).await {
                    Ok(_) => {
                        println!("Set leverage for {} to {}", k, leverage);
                    }
                    Err(_) => {
                        println!("Failed to set leverage for {}", k);
                    }
                },
                Client::Binance(cl) => match cl.set_leverage(&k, leverage as u16).await {
                    Ok(_) => {
                        println!("Set leverage for {} to {}", k, leverage);
                    }
                    Err(_) => {
                        println!("Failed to set leverage for {}", k);
                    }
                },
            }

            // Insert a new `QuoteGenerator` instance into the HashMap.
            hash.insert(
                k.clone(),
                QuoteGenerator::new(
                    v,
                    asset,
                    leverage,
                    orders_per_side,
                    final_order_distance,
                    rate_limit,
                ),
            );
        }

        hash

        // Return the populated HashMap.
    }

    /// Updates the features of the market maker based on the provided data.
    ///
    /// # Arguments
    ///
    /// * `data` - The market data containing the trades and order books.
    /// * `depth` - The depths at which to calculate imbalance and spread.
    /// * `use_wmid` - Whether to use the weighted mid price for determining skew or not.
    /// * `tick_window` - The number of ticks to consider when calculating `avg_trade_price`.
    fn update_features(&mut self, data: MarketMessage, depth: Vec<usize>) {
        // Iterate over each market message and update the features.
        match data {
            // Update features for Bybit messages.
            MarketMessage::Bybit(v) => {
                // Update the current trades with the received trades.
                for (k, t) in v.trades {
                    self.curr_trades.insert(k, t);
                }

                // Update the features for each order book.
                for (k, b) in v.books {
                    // Get the feature for the current symbol.
                    let feature = self.features.get_mut(&k).unwrap();

                    // Get the previous book, trades, and average trade price.
                    let prev_book = self.old_books.get(&k);
                    let prev_trade = self.old_trades.get(&k);
                    let prev_avg = self.prev_avg_trade_price.get(&k);
                    let curr_trade = self.curr_trades.get(&k);

                    // Update the feature if all previous data is available.
                    if let (Some(book), Some(p_trades), Some(p_avg), Some(curr_trades)) =
                        (prev_book, prev_trade, prev_avg, curr_trade)
                    {
                        feature.update(&b, book, curr_trades, p_trades, p_avg, depth.clone());
                    }

                    // Update the old books and average trade prices.
                    self.old_books.insert(k.clone(), b.clone());
                    self.prev_avg_trade_price
                        .insert(k.clone(), feature.avg_trade_price);
                }

                // Update the old trades.
                self.old_trades = self.curr_trades.clone();
            }

            // Update features for Binance messages.
            MarketMessage::Binance(v) => {
                // Update the current trades with the received trades.
                for (k, t) in v.trades {
                    self.curr_trades.insert(k, t);
                }

                // Update the features for each order book.
                for (k, b) in v.books {
                    // Get the feature for the current symbol.
                    let feature = self.features.get_mut(&k).unwrap();

                    // Get the previous book, trades, and average trade price.
                    let prev_book = self.old_books.get(&k);
                    let prev_trade = self.old_trades.get(&k);
                    let prev_avg = self.prev_avg_trade_price.get(&k);
                    let curr_trade = self.curr_trades.get(&k);

                    // Update the feature if all previous data is available.
                    if let (Some(book), Some(p_trades), Some(p_avg), Some(curr_trades)) =
                        (prev_book, prev_trade, prev_avg, curr_trade)
                    {
                        feature.update(&b, book, curr_trades, p_trades, p_avg, depth.clone());
                    }

                    // Update the old books and average trade prices.
                    self.old_books.insert(k.clone(), b);
                    self.prev_avg_trade_price.insert(k, feature.avg_trade_price);
                }

                // Update the old trades.
                self.old_trades = self.curr_trades.clone();
            }
        }
    }

    /// Update the strategy with new market data and private data.
    ///
    /// # Arguments
    ///
    /// * `data` - The new market data.
    /// * `private_data` - The private data for each symbol.
    async fn potentially_update(
        &mut self,
        private: HashMap<String, PrivateData>,
        data: MarketMessage,
    ) {
        // Get the book, private data, skew, and imbalance for each symbol
        match data {
            // If the market data is from Bybit
            MarketMessage::Bybit(v) => {
                // Update the strategy for each symbol
                for (symbol, book) in v.books {
                    // Get the skew and imbalance for the current symbol
                    let skew = self.features.get(&symbol).unwrap().skew;
                    // Get the symbol quoter for the current symbol
                    let symbol_quoter = self.generators.get_mut(&symbol).unwrap();

                    if let Some(private_data) = private.get(&symbol) {
                        // Update the symbol quoter
                        symbol_quoter
                            .update_grid(private_data.clone(), skew, book, symbol)
                            .await;
                    }
                }
            }
            // If the market data is from Binance
            MarketMessage::Binance(v) => {
                // Update the strategy for each symbol
                for (symbol, book) in v.books {
                    // Get the skew and imbalance for the current symbol
                    let skew = self.features.get(&symbol).unwrap().skew;

                    // Get the symbol quoter for the current symbol
                    let symbol_quoter = self.generators.get_mut(&symbol).unwrap();

                    if let Some(private_data) = private.get(&symbol) {
                        // Update the symbol quoter
                        symbol_quoter
                            .update_grid(private_data.clone(), skew, book, symbol)
                            .await;
                    }
                }
            }
        }
    }

    pub fn set_spread_bps_input(&mut self) {
        for (k, v) in self.generators.iter_mut() {
            let prompt = format!("Note: This is also used a max. deviation before replacement. \n Please enter spread for {} in bps: ", k);
            let spread = watch(&prompt).parse::<f64>().unwrap();
            v.set_spread(spread);
        }
    }

    /// Sets the spread for each generator using values from a TOML configuration file.
    ///
    /// # Arguments
    ///
    /// * `bps` - A vector of f64 values representing the spread in basis points for each generator.
    ///
    /// # Note
    ///
    /// This function assumes that the order of values in `bps` corresponds to the order of generators.
    pub fn set_spread_toml(&mut self, bps: Vec<f64>) {
        let mut index = 0;
        for (_, v) in self.generators.iter_mut() {
            // Set the spread for the current generator
            v.set_spread(bps[index]);
            // Move to the next spread value
            index += 1;
        }
    }
}
