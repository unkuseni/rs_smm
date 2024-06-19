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
    state.add_symbols(["1000BEERUSDT"].to_vec());
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        ss::load_data(state, sender).await;
    });

    dub(receiver).await;
}

async fn dub(mut receiver: mpsc::UnboundedReceiver<SharedState>) {
    let mut prev_books: HashMap<String, LocalBook> = HashMap::new();
    let mut prev_trades = HashMap::new();
    let mut prev_avgs = HashMap::new();
    let mut features_map = HashMap::new();

    while let Some(v) = receiver.recv().await {
        let bybit_market = &v.markets[0]; // Use reference
        let depth: Vec<usize> = vec![5, 50];
        let trades = match bybit_market {
            MarketMessage::Bybit(b) => &b.trades,
            MarketMessage::Binance(b) => &b.trades,
        };
        let trade_map: HashMap<_, _> = trades.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

        for k in &v.symbols {
            features_map
                .entry(k.to_string())
                .or_insert_with(Engine::new);
        }
        if let MarketMessage::Bybit(bybit_market) = bybit_market {
            for (symbol, book) in &bybit_market.books {
                let prev_book = prev_books.get(symbol);
                let prev_trade = prev_trades.get(symbol);
                let trade = trade_map.get(symbol).unwrap(); // Consider handling unwrap more gracefully
                let engine = features_map.get_mut(symbol).unwrap(); // Consider handling unwrap more gracefully

                if let (Some(b), Some(t), Some(avg)) =
                    (prev_book, prev_trade, prev_avgs.get(symbol))
                {
                    engine.update(book, b, trade, t, avg, depth.clone(), 1000);
                }

                if prev_books.get(symbol).is_none() {
                    prev_books.insert(symbol.clone(), book.clone());
                    prev_avgs.insert(symbol.clone(), engine.avg_trade_price);
                } else if book.last_update - prev_book.unwrap().last_update >= 100 {
                    println!(
                        "Symbol: {:#?}, Imb: {:#?}, Voi: {:#?}  wmid: {:#?} skew: {:#?}",
                        symbol,
                        engine.imbalance_ratio,
                        engine.voi,
                        engine.expected_return * 10000.0,
                        engine.skew
                    );
                    prev_books.insert(symbol.clone(), book.clone());
                    prev_avgs.insert(symbol.clone(), engine.avg_trade_price);
                } else {
                }
            }
        }
        for (symbol, trade) in trades {
            prev_trades.insert(symbol.clone(), trade.clone());
        }
    }
}
