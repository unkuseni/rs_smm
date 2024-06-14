use std::collections::HashMap;
use rs_smm::features::imbalance::voi;

use rs_smm::features::impact::avg_trade_price;
use rs_smm::features::impact::mid_price_basis;
use rs_smm::features::impact::price_impact;
use skeleton::exchanges::exchange::MarketMessage;
use skeleton::ss;
use skeleton::ss::SharedState;
use skeleton::util::helpers::Round;
use skeleton::util::localorderbook::LocalBook;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {

    let mut state = ss::SharedState::new("bybit");
    state.add_symbols(["MEWUSDT", "NOTUSDT", "JASMYUSDT"].to_vec());
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        ss::load_data(state, sender).await;
    });

    dub(receiver).await;
}



async fn dub(mut receiver: mpsc::UnboundedReceiver<SharedState>) {
    // Initialize a HashMap to store the previous LocalBook for each market.
    let mut prev_books: HashMap<String, LocalBook> = HashMap::new();
    let mut prev_trades = HashMap::new();
    let mut prev_avgs = HashMap::new();

    while let Some(v) = receiver.recv().await {
        let bybit_market = v.markets[0].clone();

        let trades = match bybit_market.clone() {
            MarketMessage::Bybit(b) => b.trades,
            MarketMessage::Binance(b) => b.trades,
        };
        let trade_map = {
            let mut map = HashMap::new();
            for v in trades.clone() {
                map.insert(v.0.clone(), v.1.clone());
            }
            map
        };

        if let MarketMessage::Bybit(bybit_market) = bybit_market {
            for (k, v) in bybit_market.books.iter().enumerate() {
                // Get the previous LocalBook for this market, if it exists.
                let prev_book = prev_books.get(&v.0);
                let prev_trade = prev_trades.get(&v.0);
                let trade = trade_map.get(&v.0).unwrap();
                // Calculate the VOI, if a previous LocalBook exists.
                let voi_value = match prev_book {
                    Some(prev_book) => voi(v.1.clone(), prev_book.clone(), Some(5)),
                    None =>  0.0, // Or some other default value.
                };
                let prev_avg = match prev_avgs.get(&v.0) {
                    Some(prev_avg) => *prev_avg,
                    None => 0.0,
                };

                let avg_trd_price =
                    avg_trade_price(v.1.mid_price, prev_trade, trade, prev_avg, 300);
                let mid_basis = match prev_book {
                    Some(prev_book) => {
                        mid_price_basis(prev_book.mid_price, v.1.mid_price, avg_trd_price)
                    }
                    None => 0.0,
                };
                let price_impact = match prev_book {
                    Some(prev_book) => price_impact(v.1.clone(), prev_book.clone(), Some(5)),
                    None => 0.0, // Or some other default value.
                };

                println!(
                    "Bybit Imbalance data: \n{:#?}, {:.5} {:#?} {:#?}",
                    v.0, v.1.mid_price, price_impact, mid_basis
                );

                // Store the current LocalBook as the previous LocalBook for the next iteration.
                prev_books.insert(v.0.clone(), v.1.clone());
                prev_avgs.insert(v.0.clone(), avg_trd_price);
            }
        }
        for v in trades {
            prev_trades.insert(v.0, v.1);
        }
    }
}
