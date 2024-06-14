use bybit::model::WsTrade;
use skeleton::util::localorderbook::LocalBook;
use skeleton::{exchanges::exchange::MarketMessage, ss::SharedState};
use std::collections::{HashMap, VecDeque};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::features::engine::Engine;

pub struct MarketMaker {
    pub state: SharedState,
    pub old_books: HashMap<String, LocalBook>,
    pub old_trades: HashMap<String, VecDeque<WsTrade>>,
    pub features: HashMap<String, Engine>,
}

impl MarketMaker {
    pub fn new(ss: SharedState) -> Self {
        MarketMaker {
            state: ss,
            old_books: HashMap::new(),
            old_trades: HashMap::new(),
            features: HashMap::new(),
        }
    }

    pub async fn start_loop(&mut self, mut receiver: UnboundedReceiver<SharedState>) {
        while let Some(data) = receiver.recv().await {
            match data.exchange {
                "bybit" | "binance" => {
                    // build features for each symbol here
                    self.features = self.build_features(data.markets[0].clone());
                }
                "both" => {}
                _ => {
                    // Panic if it dooesnt match
                }
            }
        }
    }

    fn build_features(&self, market: MarketMessage) -> HashMap<String, Engine> {
        let mut mock_hash: HashMap<String, Engine> = HashMap::new();
        match market {
            MarketMessage::Bybit(bybit_data) => {
                for (index, (symbol, book)) in bybit_data.books.iter().enumerate() {
                    let (prev_book, prev_trades) =
                        (self.old_books.get(symbol), self.old_trades.get(symbol));
                    let mock_engine = Engine::new();
                    mock_hash.insert(symbol.to_string(), mock_engine);
                }
            }
            MarketMessage::Binance(binance_data) => {
                for (index, (symbol, book)) in binance_data.books.iter().enumerate() {
                    let (prev_book, prev_trades) =
                        (self.old_books.get(symbol), self.old_trades.get(symbol));
                    let mock_engine = Engine::new();
                    mock_hash.insert(symbol.to_string(), mock_engine);
                }
            }
        }

        mock_hash
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
