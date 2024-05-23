pub mod exchanges;
pub mod ss;
pub mod util;

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {

    use std::{sync::Arc, time::Duration};

    use tokio::{
        sync::{mpsc, Mutex},
        task,
        time::Instant,
    };

    use crate::{
        exchanges::{
            ex_binance::{BinanceClient, BinanceMarket},
            ex_bybit::BybitClient,
        },
        util::logger::Logger,
    };

    use self::util::helpers::read_toml;

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
        let symbol = vec!["SKLUSDT"];
        let clone_symbol = symbol.clone();
        let (tx2, mut rx2) = mpsc::unbounded_channel::<BinanceMarket>();
        let bub_2 = BinanceClient::init(api_key, api_secret);
        let symbol_2 = vec!["SKLUSDT"];
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
                    println!("Bybit Market data: {:#?}, {:#?}", clone_symbol[0], depth);
                }
                Some(v) = rx2.recv() => {
                    let depth = v.books[0].1.get_bba();
                    println!("Binance Market data: {:#?}, {:#?}", clone_symbol_2[0], depth);
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
        let symbol = vec!["ETHUSDT"];
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
    async fn test_agg() {
        let bub = BinanceClient::init("key".to_string(), "secret".to_string());
        let (tx, mut rx) = mpsc::unbounded_channel();
        tokio::task::spawn_blocking(move || {
            let _ = bub.ws_aggtrades(vec!["BTCUSDT", "ETHUSDT", "SKLUSDT", "MATICUSDT"], tx);
        });
        while let Some(v) = rx.recv().await {
            println!("Aggtrade data: {:#?}", v);
        }
    }

    #[tokio::test]
    async fn test_book() {
        let bub = BinanceClient::init("key".to_string(), "secret".to_string());
        let (tx, mut rx) = mpsc::unbounded_channel();
        tokio::task::spawn_blocking(move || {
            let _ = bub.ws_best_book(vec!["LQTYUSDT"], tx);
        });
        while let Some(v) = rx.recv().await {
            println!("Aggtrade data: {:#?}", v);
        }
    }

    #[tokio::test]
    async fn test_user_stream() {
        let bub = BinanceClient::init(
            "N4qNFLgddNxqwG7tWu4b6VdgCSdIXPzFDyEfu48AkCjN3bLvXCWaRvhEcy8qX6dD".to_string(),
            "secret".to_string(),
        );
        let (tx, mut rx) = mpsc::unbounded_channel();
        tokio::task::spawn_blocking(move || {
            let _ = bub.private_subscribe(tx);
        });
        while let Some(v) = rx.recv().await {
            for (k, d) in v.orders.iter() {
                println!("Private data: {:#?}, {:#?}", k, d);
            }
        }
    }

    #[tokio::test]
    async fn test_priv() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let api_key = "Gf00VfhZXp11aGSxoV".to_string();
        let api_secret = "cnafpFvqeAC2dUPyeFP61QwYeAJiPdbdNgMX".to_string();
        let bub = BybitClient::init(api_key, api_secret);
        tokio::spawn(async move {
            bub.private_subscribe(tx).await;
        });
        while let Some(v) = rx.recv().await {
            println!("Private data: {:#?}", v);
        }
    }

    #[tokio::test]
    async fn test_fee() {
        let api_key = "Gf00VfhZXp11aGSxoV".to_string();
        let api_secret = "cnafpFvqeAC2dUPyeFP61QwYeAJiPdbdNgMX".to_string();
        let rate = task::spawn_blocking(move || {
            let api_key2 =
                "BOswZzt8n49xqKhZu2KYxObLLXf6iOVpyjLtUbmNcZhTMIuDam0Jn7AArzOlzVQM".to_string();
            let api_secret2 =
                "D0JlW0Uf0SBkRgNmGTNMymgwI2BVQylNqkdqzMqpE74dXRE5SAL4o85V7LivGfSx".to_string();
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
        let mut state = ss::SharedState::new();
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let instant = Instant::now();
        let wrapped = Arc::new(Mutex::new(state));
        tokio::spawn(async move {
            ss::load_data(wrapped, sender).await;
        });
        while let Some(v) = receiver.recv().await {
            if instant.elapsed() > Duration::from_secs(60) {
                println!("Shared State: {:#?}", v);
                break;
            }
        }
    }
}
