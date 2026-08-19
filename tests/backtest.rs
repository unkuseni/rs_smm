//! End-to-end test of the recording -> replay -> simulated trading pipeline.

use std::collections::HashMap;

use bybit::{Ask, Bid, TickDirection, WsTrade};
use rs_smm::backtest;
use skeleton::util::helpers::{Config, StrategyConfig};
use skeleton::util::localorderbook::LocalBook;
use skeleton::util::recorder::Recorder;
use skeleton::exchanges::exchange::MarketEvent;

fn make_config(record: &str) -> Config {
    Config {
        exchange: "bybit".to_string(),
        symbols: vec!["BTCUSDT".to_string()],
        api_keys: vec![],
        balances: vec![("BTCUSDT".to_string(), 1000.0)],
        leverage: 10.0,
        orders_per_side: 2,
        final_order_distance: 5.0,
        depths: vec![3],
        rate_limit: 1000,
        tick_window: 5,
        bps: HashMap::from([("BTCUSDT".to_string(), 10.0)]),
        record: Some(record.to_string()),
        state_file: None,
        strategy: StrategyConfig::default(),
        turso: skeleton::util::helpers::TursoConfig::default(),
    }
}

fn meta_book() -> LocalBook {
    let mut book = LocalBook::new();
    book.tick_size = 0.01;
    book.lot_size = 0.001;
    book.min_order_size = 0.001;
    book.min_notional = 1.0;
    book.post_only_max = 1000.0;
    book
}

fn write_recording(path: &str) {
    let book = meta_book();
    let books = vec![("BTCUSDT".to_string(), &book)];
    let mut recorder = Recorder::new(path, "bybit", 0, &books).expect("create recorder");

    // Walk the price up and down, printing trades at the ask (buys) and at
    // the bid (sells), with a book delta before each trade.
    let mut ts: u64 = 0;
    let mut price = 100.0;
    for i in 0..120 {
        let dir = if (i / 20) % 2 == 0 { 1.0 } else { -1.0 };
        price += dir * 0.01;
        ts += 10;
        recorder
            .record(&MarketEvent::Book {
                symbol: "BTCUSDT".to_string(),
                bids: vec![Bid { price, qty: 5.0 }],
                asks: vec![Ask { price: price + 0.05, qty: 5.0 }],
                timestamp: ts,
                bba: true,
            })
            .expect("record book");
        ts += 1;
        let buy = i % 2 == 0;
        recorder
            .record(&MarketEvent::Trade {
                symbol: "BTCUSDT".to_string(),
                trade: WsTrade::new(
                    ts,
                    "BTCUSDT",
                    if buy { "Buy" } else { "Sell" },
                    0.5,
                    if buy { price + 0.05 } else { price },
                    TickDirection::PlusTick,
                    &format!("t{}", i),
                    false,
                ),
            })
            .expect("record trade");
    }
    recorder.finish().expect("flush");
}

#[tokio::test]
async fn replay_synthetic_recording() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "rs_smm_backtest_{}.rec",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let path_str = path.to_str().unwrap().to_string();
    write_recording(&path_str);

    let config = make_config(&path_str);
    let report = backtest::run(&path_str, &config)
        .await
        .expect("backtest should run");

    assert_eq!(report.symbols.len(), 1);
    let s = &report.symbols[0];
    assert_eq!(s.symbol, "BTCUSDT");
    assert!(s.book_updates > 0, "book updates were applied");
    assert!(s.trades > 0, "trades were applied");
    assert!(s.grid_refreshes > 0, "the grid was placed after warmup");
    assert!(s.total_pnl.is_finite());
    assert!(s.sharpe.is_finite());

    let _ = std::fs::remove_file(&path);
}
