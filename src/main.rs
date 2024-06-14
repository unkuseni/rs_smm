use rs_smm::features::engine::Engine;
use std::collections::HashMap;

use skeleton::exchanges::exchange::MarketMessage;
use skeleton::ss;
use skeleton::ss::SharedState;
use skeleton::util::localorderbook::LocalBook;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    let mut state = ss::SharedState::new("bybit");
    state.add_symbols(["NOTUSDT"].to_vec());
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
    let mut features_map = HashMap::new();

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

        for k in v.symbols {
            features_map.insert(k.to_string(), Engine::new());
        }
        if let MarketMessage::Bybit(bybit_market) = bybit_market {
            for (_, v) in bybit_market.books.iter().enumerate() {
                // Get the previous LocalBook for this market, if it exists.
                let prev_book = prev_books.get(&v.0);
                let prev_trade = prev_trades.get(&v.0);
                let trade = trade_map.get(&v.0).unwrap();
                // Calculate the VOI, if a previous LocalBook exists.
                let engine = features_map.get_mut(&v.0).unwrap();
                if let (Some(b), Some(t), Some(avg)) = (prev_book, prev_trade, prev_avgs.get(&v.0))
                {
                    engine.update(&v.1, b, trade, t, avg, Some(5));
                }

                println!(
                    "Symbol: {:#?}, mid_price: {:.6}, voi: {:#?}, imbalance_ratio: {:#?}, trade_imb: {:#?}",
                    v.0, v.1.mid_price, engine.voi, engine.imbalance_ratio, engine.trade_imb
                );

                // Store the current LocalBook as the previous LocalBook for the next iteration.
                prev_books.insert(v.0.clone(), v.1.clone());
                prev_avgs.insert(v.0.clone(), engine.avg_trade_price);
            }
        }
        for v in trades {
            prev_trades.insert(v.0, v.1);
        }
    }
}
