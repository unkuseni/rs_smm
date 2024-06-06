use rs_smm::features::imbalance::imbalance_ratio;
use rs_smm::features::imbalance::imbalance_ratio_at_depth;
use rs_smm::features::imbalance::trade_imbalance;
use rs_smm::features::imbalance::voi;
use skeleton::exchanges::ex_bybit::BybitMarket;
use skeleton::exchanges::exchange::MarketMessage;
use skeleton::exchanges::exchange::ProcessTrade;
use skeleton::ss;
use skeleton::util::localorderbook::LocalBook;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant};
#[tokio::main]
async fn main() {
    let mut state = ss::SharedState::new("bybit");
    state.add_symbols(["BTCUSDT", "ETHUSDT"].to_vec());
    let (sender, mut receiver) = mpsc::unbounded_channel();

    let wrapped = Arc::new(Mutex::new(state));
    tokio::spawn(async move {
        ss::load_data(wrapped, sender).await;
    });
    // let mut old_mk = None;

    while let Some(v) = receiver.recv().await {
        // handle_markets(v.clone().markets, old_mk);
        // old_mk = Some(v.markets);
        let bybit_market = v.markets[0].clone();

        let trades = match bybit_market.clone() {
            MarketMessage::Bybit(b) => b.trades,
            _ => panic!("Not bybit market"),
        };


        if let MarketMessage::Bybit(bybit_market) = bybit_market {
            for (k, v) in bybit_market.books.iter().enumerate() {
                println!(
                    "Bybit Imbalance data: \n{:#?}, {:.5} {:#?} {:#?} {:#?}",
                    v.0,
                    imbalance_ratio_at_depth(v.1.clone(), 5),
                    v.1.mid_price,
                    trades[k].1.len(),   
                    trade_imbalance(trades[k].clone()),     
                );
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
