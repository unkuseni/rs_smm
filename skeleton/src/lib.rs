pub mod exchanges;
pub mod ss;
pub mod util;

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {

    use std::time::Duration;

    use binance::{api::Binance, futures::general::FuturesGeneral};
    use exchanges::exchange::PrivateData;
    use tokio::{sync::mpsc, task, time::Instant};

    use crate::{
        exchanges::{
            ex_binance::{BinanceClient, BinanceMarket},
            ex_bybit::BybitClient,
        },
        util::logger::Logger,
    };

    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }

    #[tokio::test]
    async fn test_orderbook_both() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let api_key = "key".to_string();
        let api_secret = "secret".to_string();
        let bub = BybitClient::init(api_key.clone(), api_secret.clone());
        let symbol = vec!["NOTUSDT".to_string()];
        let clone_symbol = symbol.clone();
        let (tx2, mut rx2) = mpsc::unbounded_channel::<BinanceMarket>();
        let bub_2 = BinanceClient::init(api_key, api_secret);
        let symbol_2 = vec!["NOTUSDT".to_string()];
        let clone_symbol_2 = symbol_2.clone();

        tokio::spawn(async move {
            bub.market_subscribe(symbol, tx).await;
        });

        let binance_task = tokio::task::spawn_blocking(move || {
            bub_2.market_subscribe(symbol_2, tx2);
        });

        loop {
            tokio::select! {
                Some(v) = rx.recv() => {
                    let depth = v.books[0].1.get_bba();
                    let spread = v.books[0].1.get_spread();
                    let bps_spread = v.books[0].1.get_spread_in_bps();
                    println!("Bybit Market data: {:#?}, {:#?} {:#?}, {:#?}", clone_symbol[0], depth, spread, bps_spread);
                }
                Some(v) = rx2.recv() => {
                    let depth = v.books[0].1.get_bba();
                    let spread = v.books[0].1.get_spread();
                    let bps_spread = v.books[0].1.get_spread_in_bps();
                    println!("Binance Market data: {:#?}, {:#?} {:#?}, {:#?}", clone_symbol_2[0], depth, spread, bps_spread);
                }
                else => break,
            }
        }

        binance_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_orderbook_bin() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut data;
        let api_key = "key".to_string();
        let api_secret = "secret".to_string();
        let bub = BinanceClient::init(api_key, api_secret);
        let symbol = vec!["ETHUSDT".to_string()];
        let symbol_clone = symbol.clone();

        let _webs = tokio::task::spawn_blocking(move || {
            let _ = bub.market_subscribe(symbol, tx);
        });
        let mut counter = 0;

        while let Some(v) = rx.recv().await {
            data = v;
            let depth = data.books[0].1.get_book_depth(3);
            println!("Market data: {:#?}, {:#?}", symbol_clone[0], depth);
            counter += 1;
            if counter == 200 {
                println!("Market data: {:#?}", data);
                break;
            }
        }
    }

    #[tokio::test]
    async fn test_time() {
        let api_key = "key".to_string();
        let api_secret = "secret".to_string();
        let bub = BybitClient::init(api_key, api_secret);
        let time = bub.exchange_time().await;
        println!("Time: {}", time);
    }

    #[tokio::test]
    async fn test_bin_time() {
        let api_key = "key".to_string();
        let api_secret = "secret".to_string();
        let bub = BinanceClient::init(api_key, api_secret);
        let _ = task::spawn_blocking(move || {
            let time = bub.exchange_time();
            println!("Time: {}", time);
        })
        .await;
    }


    #[tokio::test]
    async fn test_user_stream() {
        let bub = BinanceClient::init("api".to_string(), "secret".to_string());
        let (tx, mut rx) = mpsc::unbounded_channel();
        let symbol = "BTCUSDT".to_string();
        tokio::task::spawn_blocking(move || {
            let _ = bub.private_subscribe(tx, symbol);
        });
        while let Some(v) = rx.recv().await {
            match v.data {
                PrivateData::Binance(v) => {
                    for (k, d) in v.orders.iter() {
                        println!("Private data: {:#?}, {:#?}", k, d);
                    }
                }
                _ => {}
            }
        }
    }

    #[tokio::test]
    async fn test_priv() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let api_key = "api".to_string();
        let api_secret = "secret".to_string();
         let symbol = "BTCUSDT".to_string();
        let bub = BybitClient::init(api_key, api_secret);
        tokio::spawn(async move {
            bub.private_subscribe(tx, symbol).await;
        });
        while let Some(v) = rx.recv().await {
            println!("Private data: {:#?}", v.data);
        }
    }

    #[tokio::test]
    async fn test_fee() {
        let _api_key = "api".to_string();
        let _api_secret = "secret".to_string();
        let _rate = task::spawn_blocking(move || {
            let api_key2 = "api".to_string();
            let api_secret2 = "secret".to_string();
            let bub = BinanceClient::init(api_key2, api_secret2);
            bub.fee_rate();
        })
        .await;
    }

    #[tokio::test]
    pub async fn test_log() {
        let logger = Logger;
        logger.info("info");
        logger.success("success");
        logger.debug("debug");
        logger.warning("warning");
        logger.error("error");
    }

    #[tokio::test]
    pub async fn test_new_state() {
        let exchange = "binance".to_string();
        let mut state = ss::SharedState::new(exchange);
        state.add_symbols(["SKLUSDT".to_string(), "MATICUSDT".to_string()].to_vec());
        let (sender, mut receiver) = mpsc::unbounded_channel::<ss::SharedState>();
        let instant = Instant::now();
        tokio::spawn(async move {
            ss::load_data(state, sender).await;
        });
        while let Some(v) = receiver.recv().await {
            println!("Shared State: {:#?}", v.exchange);
            v.logging.info("Received state");
            if instant.elapsed() > Duration::from_secs(60) {
                println!("Shared State: {:#?}", v.markets[0]);
                break;
            }
        }
    }

    #[test]
    pub fn test_general() {
        let data_cl: FuturesGeneral = Binance::new(None, None);
        match data_cl.get_symbol_info("SKLUSDT") {
            Ok(v) => println!("{:#?}", v),
            Err(e) => println!("{:#?}", e),
        }
    }
}
