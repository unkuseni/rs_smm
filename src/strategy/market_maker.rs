use bybit::model::WsTrade;
use skeleton::exchanges::exchange::{ExchangeClient, PrivateData};
use skeleton::util::localorderbook::LocalBook;
use skeleton::{exchanges::exchange::MarketMessage, ss::SharedState};
use tokio::time::{self, Duration};
use std::collections::{HashMap, VecDeque};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::features::engine::Engine;
use crate::features::imbalance::imbalance_ratio;
use crate::trader::quote_gen::QuoteGenerator;

pub struct MarketMaker {
    pub features: HashMap<String, Engine>,
    pub old_books: HashMap<String, LocalBook>,
    pub old_trades: HashMap<String, VecDeque<WsTrade>>,
    pub curr_trades: HashMap<String, VecDeque<WsTrade>>,
    pub prev_avg_trade_price: HashMap<String, f64>,
    pub generators: HashMap<String, QuoteGenerator>,
}

impl MarketMaker {
    pub fn new(ss: SharedState, assets: HashMap<String, f64>) -> Self {
        MarketMaker {
            features: MarketMaker::build_features(ss.symbols.clone()),
            old_books: HashMap::new(),
            old_trades: HashMap::new(),
            curr_trades: HashMap::new(),
            prev_avg_trade_price: HashMap::new(),
            generators: MarketMaker::build_generators(ss.clients, assets),
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
        let mut interval = time::interval(Duration::from_millis(500));
        // Continuously receive and process shared state updates.
        while let Some(data) = receiver.recv().await {
            // Match the exchange in the received data.
            match data.exchange {
                "bybit" | "binance" => {
                    // Update features with the first market data in the received data.
                    self.update_features(data.markets[0].clone(), vec![5, 50]);
                    if send > 200 {
                        self.potentially_update(data.markets[0].clone(), data.private.clone())
                            .await;
                    } else {
                        interval.tick().await;
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
    fn build_features(symbol: Vec<&str>) -> HashMap<String, Engine> {
        // Create a new HashMap to store the features.
        let mut hash: HashMap<String, Engine> = HashMap::new();

        // Iterate over each symbol and insert a new `Engine` instance into the HashMap.
        for v in symbol {
            // Convert the symbol name to a string and insert it into the HashMap.
            hash.insert(v.to_string(), Engine::new());
        }

        // Return the populated HashMap.
        hash
    }

    fn build_generators(
        clients: HashMap<String, ExchangeClient>,
        assets: HashMap<String, f64>,
    ) -> HashMap<String, QuoteGenerator> {
        let mut hash: HashMap<String, QuoteGenerator> = HashMap::new();
        for (k, v) in clients {
            hash.insert(
                k.clone(),
                QuoteGenerator::new(v, assets.get(&k).unwrap().clone(), 15.0),
            );
        }
        hash
    }

    fn update_features(&mut self, data: MarketMessage, depth: Vec<usize>) {
        // TODO: update features
        match data {
            MarketMessage::Bybit(v) => {
                for (k, t) in v.trades {
                    self.curr_trades.insert(k, t);
                }
                // TODO
                for (k, b) in v.books {
                    let feature = self.features.get_mut(&k).unwrap();
                    let prev_book = self.old_books.get(&k);
                    let prev_trade = self.old_trades.get(&k);
                    let prev_avg = self.prev_avg_trade_price.get(&k);
                    let curr_trade = self.curr_trades.get(&k);
                    if let (Some(book), Some(p_trades), Some(p_avg), Some(curr_trades)) =
                        (prev_book, prev_trade, prev_avg, curr_trade)
                    {
                        feature.update(&b, book, curr_trades, p_trades, p_avg, depth.clone(), 1000);
                    }
                    self.old_books.insert(k.clone(), b.clone());
                    self.prev_avg_trade_price
                        .insert(k.clone(), feature.avg_trade_price);
                }

                self.old_trades = self.curr_trades.clone();
            }
            MarketMessage::Binance(v) => {
                for (k, t) in v.trades {
                    self.curr_trades.insert(k, t);
                }
                // TODO
                for (k, b) in v.books {
                    let feature = self.features.get_mut(&k).unwrap();
                    let prev_book = self.old_books.get(&k);
                    let prev_trade = self.old_trades.get(&k);
                    let prev_avg = self.prev_avg_trade_price.get(&k);
                    let curr_trade = self.curr_trades.get(&k);
                    if let (Some(book), Some(p_trades), Some(p_avg), Some(curr_trades)) =
                        (prev_book, prev_trade, prev_avg, curr_trade)
                    {
                        feature.update(&b, book, curr_trades, p_trades, p_avg, depth.clone(), 1000);
                    }
                    self.old_books.insert(k.clone(), b);
                    self.prev_avg_trade_price.insert(k, feature.avg_trade_price);
                }
                self.old_trades = self.curr_trades.clone();
            }
        }
    }

    async fn potentially_update(
        &mut self,
        data: MarketMessage,
        private_data: HashMap<String, PrivateData>,
    ) {
        // TODO: get book, private, skew, and imbalance

        match data {
            MarketMessage::Bybit(v) => {
                // TODO
                for (symbol, book) in v.books {
                    let wallet = {
                        match private_data.get(&symbol) {
                            Some(v) => v.clone(),
                            None => {
                                panic!("Private data for {} not found", symbol);
                            }
                        }
                    };
                    let skew = self.features.get(&symbol).unwrap().skew;
                    let imbalance = imbalance_ratio(&book, Some(18));
                    let symbol_quoter = self.generators.get_mut(&symbol).unwrap();
                    symbol_quoter.update_grid(wallet.clone(), skew, imbalance, book, symbol);
                }
            }
            MarketMessage::Binance(v) => {
                // TODO
                for (symbol, book) in v.books {
                    let wallet = private_data.get(&symbol).unwrap();
                    let skew = self.features.get(&symbol).unwrap().skew;
                    let imbalance = imbalance_ratio(&book, Some(18));
                    let symbol_quoter = self.generators.get_mut(&symbol).unwrap();
                    symbol_quoter.update_grid(wallet.clone(), skew, imbalance, book, symbol);
                }
            }
        }
    }
}

// pub struct MarketMakerInput {
//     pub asset: String,
//     pub target_liquidity: f64, // Amount of liquidity on both sides to target
//     pub half_spread: u16,      // Half of the spread for our market making (in BPS)
//     pub max_bps_diff: u16, // Max deviation before we cancel and put new orders on the book (in BPS)
//     pub max_absolute_position_size: f64, // Absolute value of the max position we can take on
//     pub decimals: u32,     // Decimals to round to for pricing
//     pub wallet: LocalWallet, // Wallet containing private key
// }

// pub struct MarketMaker {
//     pub asset: String,
//     pub target_liquidity: f64,
//     pub half_spread: u16,
//     pub max_bps_diff: u16,
//     pub max_absolute_position_size: f64,
//     pub decimals: u32,
//     pub lower_resting: MarketMakerRestingOrder,
//     pub upper_resting: MarketMakerRestingOrder,
//     pub cur_position: f64,
//     pub latest_mid_price: f64,
//     pub info_client: InfoClient,
//     pub exchange_client: ExchangeClient,
//     pub user_address: H160,
// }

// Things for my market maker to track
// - Assets for each client
// - Target liquidity
// - base spread: Account for profit, volatility and symbol spread
// - max_position for each side 0.5
// - current position
// - resting bid or ask order
//- max-deviation before shifting orders

#[cfg(test)]
mod tests {
    use skeleton::util::logger::Logger;
    use tokio::time::Duration;

    use tokio::time;

    use super::*;

    #[tokio::test]
    async fn test_tick() {
        let mut interval = time::interval(Duration::from_millis(500));
        let log = Logger;
        let arr: VecDeque<f64> = VecDeque::with_capacity(100);
        loop {
            interval.tick().await;
            log.info("Test log");
            println!("arr.len(): {}", arr.len());
        }
    }
}
