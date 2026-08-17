const BYBIT_KEY: &str = "";
const BYBIT_SECRET: &str = "";

const BINANCE_KEY: &str = "";
const BINANCE_SECRET: &str = "";

#[cfg(test)]
mod tests {

    use std::sync::Arc;
    use std::time::Duration;

    use binance::{api::Binance, futures::general::FuturesGeneral};
    use skeleton::{
        exchanges::{
            ex_binance::BinanceClient,
            ex_bybit::BybitClient,
            exchange::{Exchange, MarketEvent, PrivateData},
        },
        ss,
        util::logger::Logger,
    };
    use tokio::{sync::mpsc, time::Instant};

    use crate::{BINANCE_KEY, BINANCE_SECRET, BYBIT_KEY, BYBIT_SECRET};

    // Tests marked #[ignore] require live exchange connectivity and/or real
    // credentials; run them explicitly with `cargo test -- --ignored`.

    #[test]
    fn test_default() {
        let client = BybitClient::default();
        assert_eq!(client.key, "");
        assert_eq!(client.secret, "");
    }

    #[test]
    fn test_init() {
        let client = BybitClient::init(BYBIT_KEY, BYBIT_SECRET);
        assert_eq!(client.key, BYBIT_KEY);
        assert_eq!(client.secret, BYBIT_SECRET);
    }

    #[ignore]
    #[tokio::test]
    async fn test_time() {
        let client = BybitClient::init(BYBIT_KEY, BYBIT_SECRET);
        let client_two = BinanceClient::init(BINANCE_KEY, BINANCE_SECRET);
        let bybit_time = client.time().await as i64;
        let binance_time = client_two.time().await as i64;
        println!(
            "Bybit Time: {:?}, Binance Time: {:?} diff: {:?}",
            bybit_time,
            binance_time,
            bybit_time - binance_time
        );
    }

    #[ignore]
    #[tokio::test]
    async fn test_fees() {
        let client = BybitClient::init(BYBIT_KEY, BYBIT_SECRET);
        let client_two = BinanceClient::init(BINANCE_KEY, BINANCE_SECRET);
        let bybit_fees = client.fees().await;
        let binance_fees = client_two.fees().await;
        println!(
            "Bybit Fees: {:?} \nBinance Fees: {:?}",
            bybit_fees, binance_fees,
        );
    }

    #[test]
    fn test_trade() {
        let client = BybitClient::init(BYBIT_KEY, BYBIT_SECRET);
        let _ = client.trader();
    }

    #[ignore]
    #[tokio::test]
    async fn test_bybit_books() {
        let client = BybitClient::init(BYBIT_KEY, BYBIT_SECRET);
        let (tx, mut rx) = mpsc::unbounded_channel::<MarketEvent>();
        let symbols = vec!["NOTUSDT".to_string(), "ETHUSDT".to_string()];
        tokio::spawn(async move {
            client.market_subscribe(symbols, tx).await;
        });

        while let Some(v) = rx.recv().await {
            println!("Market event: {:#?}", v);
        }
    }

    #[ignore]
    #[tokio::test]
    async fn test_binance_books() {
        let client = BinanceClient::init(BINANCE_KEY, BINANCE_SECRET);
        let (tx, mut rx) = mpsc::unbounded_channel::<MarketEvent>();
        let symbols = vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()];
        tokio::spawn(async move {
            client.market_subscribe(symbols, tx).await;
        });
        while let Some(v) = rx.recv().await {
            println!("Market event: {:#?}", v);
        }
    }

    #[ignore]
    #[tokio::test]
    pub async fn test_general() {
        let data_cl: FuturesGeneral = Binance::new(None, None);
        match data_cl.get_symbol_info("SKLUSDT").await {
            Ok(v) => println!("{:#?}", v),
            Err(e) => println!("{:#?}", e),
        }
    }

    #[ignore]
    #[tokio::test]
    pub async fn test_new_state() {
        let exchange = "both".to_string();
        let mut state = ss::SharedState::new(exchange);
        state.add_symbols(["SKLUSDT".to_string(), "MATICUSDT".to_string()].to_vec());
        let (sender, mut receiver) = mpsc::unbounded_channel::<Arc<ss::SharedState>>();
        let instant = Instant::now();
        tokio::spawn(async move {
            ss::load_data(state, sender).await;
        });
        while let Some(v) = receiver.recv().await {
            v.logging.info("Received state");
            if instant.elapsed() > Duration::from_secs(60) {
                break;
            }
        }
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

    #[ignore]
    #[tokio::test]
    async fn test_priv() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let api_key = "api";
        let api_secret = "secret";
        let symbol = "BTCUSDT".to_string();
        let bub = BybitClient::init(api_key, api_secret);
        tokio::spawn(async move {
            bub.private_subscribe(tx, symbol).await;
        });
        while let Some(v) = rx.recv().await {
            println!("Private data: {:#?}", v.data);
        }
    }

    #[ignore]
    #[tokio::test]
    async fn test_user_stream() {
        let bub = BinanceClient::init("api", "secret");
        let (tx, mut rx) = mpsc::unbounded_channel();
        let symbol = "BTCUSDT".to_string();
        tokio::spawn(async move {
            bub.private_subscribe(tx, symbol).await;
        });
        while let Some(v) = rx.recv().await {
            if let PrivateData::Binance(v) = v.data {
                for (k, d) in v.orders.iter() {
                    println!("Private data: {:#?}, {:#?}", k, d);
                }
            }
        }
    }

    #[ignore]
    #[tokio::test]
    async fn test_orderbook_bin() {
        let (tx, mut rx) = mpsc::unbounded_channel::<MarketEvent>();
        let api_key = "key";
        let api_secret = "secret";
        let bub = BinanceClient::init(api_key, api_secret);
        let symbol = vec!["ETHUSDT".to_string()];

        let _webs = tokio::spawn(async move {
            bub.market_subscribe(symbol, tx).await;
        });
        let mut counter = 0;

        while let Some(v) = rx.recv().await {
            println!("Market event: {:#?}", v);
            counter += 1;
            if counter == 200 {
                break;
            }
        }
    }

    #[ignore]
    #[tokio::test]
    async fn test_orderbook_both() {
        let (tx, mut rx) = mpsc::unbounded_channel::<MarketEvent>();
        let api_key = "key";
        let api_secret = "secret";
        let bub = BybitClient::init(api_key, api_secret);
        let symbol = vec!["NOTUSDT".to_string()];

        let (tx2, mut rx2) = mpsc::unbounded_channel::<MarketEvent>();
        let bub_2 = BinanceClient::init(api_key, api_secret);
        let symbol_2 = vec!["NOTUSDT".to_string()];

        tokio::spawn(async move {
            bub.market_subscribe(symbol, tx).await;
        });

        let binance_task = tokio::spawn(async move {
            bub_2.market_subscribe(symbol_2, tx2).await;
        });

        loop {
            tokio::select! {
                Some(v) = rx.recv() => {
                    println!("Bybit Market event: {:#?}", v);
                }
                Some(v) = rx2.recv() => {
                    println!("Binance Market event: {:#?}", v);
                }
                else => break,
            }
        }

        binance_task.await.unwrap();
    }
}
