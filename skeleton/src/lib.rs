/// Exchanges
pub mod exchanges;
pub mod ss;
pub mod util;

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {

    use tokio::{sync::mpsc, task};

    use crate::exchanges::{ex_binance::BinanceClient, ex_bybit::BybitClient};

    use self::exchanges::exchange::Exchange;

    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }

    #[tokio::test]
    async fn test_orderbook() {
        let (tx, mut rx) = mpsc::unbounded_channel();

        let api_key = "key".to_string();
        let api_secret = "secret".to_string();
        let bub = BybitClient::init(api_key, api_secret);
        let symbol = vec!["MATICUSDT", "ETHUSDT"];

        let _webs = tokio::spawn(async move {
            bub.market_subscribe(symbol, tx).await;
        });

        while let Some(v) = rx.recv().await {
            println!(
                "Market data: MATICUSDT {:#?} ETHUSDT {:#?}",
                v.books[0].1.get_bba(),
                v.books[1].1.get_bba()
            );
        }
    }

    #[tokio::test]
    async fn test_time() {
        let api_key = "key".to_string();
        let api_secret = "secret".to_string();
        let bub = BybitClient::init(api_key, api_secret);
        bub.exchange_time().await;
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
            bub.fee_rate2("BTCUSDT");
        })
        .await;
    }
}
