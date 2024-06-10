use std::collections::HashMap;

use rs_smm::features::imbalance::imbalance_ratio;
use rs_smm::features::imbalance::trade_imbalance;
use rs_smm::features::imbalance::voi;

use skeleton::exchanges::exchange::MarketMessage;
use skeleton::ss;
use skeleton::util::helpers::Round;
use skeleton::util::localorderbook::LocalBook;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    let mut state = ss::SharedState::new("bybit");
    state.add_symbols(["MEWUSDT", "NOTUSDT", "JASMYUSDT"].to_vec());
    let (sender, mut receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        ss::load_data(state, sender).await;
    });

    // Initialize a HashMap to store the previous LocalBook for each market.
    let mut prev_books: HashMap<String, LocalBook> = HashMap::new();

    while let Some(v) = receiver.recv().await {
        let bybit_market = v.markets[0].clone();

        let trades = match bybit_market.clone() {
            MarketMessage::Bybit(b) => b.trades,
            _ => panic!("Not bybit market"),
        };

        if let MarketMessage::Bybit(bybit_market) = bybit_market {
            for (k, v) in bybit_market.books.iter().enumerate() {
                // Get the previous LocalBook for this market, if it exists.
                let prev_book = prev_books.get(&v.0);

                // Calculate the VOI, if a previous LocalBook exists.
                let voi_value = match prev_book {
                    Some(prev_book) => voi(v.1.clone(), prev_book.clone(), Some(5)),
                    None => 0.0, // Or some other default value.
                };
        
                println!(
                    "Bybit Imbalance data: \n{:#?}, {:.5} {:.6} {:.5} {:#?}",
                    v.0,
                    imbalance_ratio(v.1.clone(), Some(5)),
                    v.1.mid_price,
                    trade_imbalance(trades[k].clone()).1,
                    voi_value
                );

                // Store the current LocalBook as the previous LocalBook for the next iteration.
                prev_books.insert(v.0.clone(), v.1.clone());
            }
        }
    }
}

pub fn handle_markets(
    markets: Vec<MarketMessage>,
    old_market: Option<Vec<MarketMessage>>,
) -> Vec<((String, LocalBook), (String, LocalBook))> {
    if let Some(v) = old_market {
        let mut new_bybit_books = Vec::new();
        let mut new_binance_books = Vec::new();
        for v in markets {
            match v {
                MarketMessage::Bybit(bybit_market) => {
                    for v in bybit_market.books {
                        new_bybit_books.push(v);
                    }
                }
                MarketMessage::Binance(binance_market) => {
                    for v in binance_market.books {
                        new_binance_books.push(v);
                    }
                }
            }
        }
        let mut both_books: Vec<((String, LocalBook), (String, LocalBook))> = Vec::new();
        for (i, j) in new_bybit_books.iter().zip(new_binance_books.iter()) {
            println!(
                "bybit book: {} {}, binance book: {} {}",
                i.0, i.1.best_bid.price, j.0, j.1.best_bid.price
            );
            both_books.push((i.clone(), j.clone()));
        }

        both_books
    } else {
        Vec::new()
    }
}
