/// Exchanges
pub mod exchanges;
pub mod util;

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {


    use bybit::model::OrderBookUpdate;
    use tokio::sync::mpsc;

    use crate::util::localorderbook::LocalBook;

    use self::exchanges::{
        exchange::{Exchange, Exchanges},
        normalized::Normalized,
    };

    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }

    #[tokio::test]
    async fn test_orderbook() {
        let (tx, mut rx) = mpsc::channel(1000);

        let api_key = "key".to_string();
        let api_secret = "secret".to_string();
        let exchange = Exchanges::Bybit;
        let bub = Normalized::init(api_key, api_secret, exchange);
        let symbol = "CEEKUSDT";
        let mut book = LocalBook::new();

        let _webs = tokio::spawn(async move {
            bub.orderbook(symbol, tx).await;
        });
        while let Some(OrderBookUpdate {
            topic,
            data,
            timestamp,
            ..
        }) = rx.recv().await
        {
            if topic == format!("orderbook.1.{}", symbol) {
                book.update_bba(data.bids, data.asks, timestamp);
            } else {
                book.update(data.bids, data.asks, timestamp);
            }
            let mid_price = (book.get_best_ask().price + book.get_best_bid().price) / 2.0;
            let _imbalance = (book.get_best_bid().qty - book.get_best_ask().qty)
                / (book.get_best_ask().qty + book.get_best_bid().qty);
            let fees = fee_percent(mid_price, 0.04);
            let spread = book.best_ask.price - book.best_bid.price;
            let arb = spread - fees;
            println!(
                "BBA: {} \nBest Ask: {:#?} \nBest Bid: {:#?} \nSpread: {:.4}, Arb: {:.4}",
                timestamp,
                book.get_best_ask().price,
                book.get_best_bid().price,
                book.best_ask.price - book.best_bid.price,
                arb
            );
        }
    }

    fn fee_percent(value: f64, percent: f64) -> f64 {
        (percent / 100.0) * value
    }

    #[tokio::test]
    async fn test_kline_data() {
        let api_key = "key".to_string();
        let api_secret = "secret".to_string();
        let (tx, mut rx) = mpsc::channel(100);
        let exchange = Exchanges::Bybit;
        let bub = Normalized::init(api_key, api_secret, exchange);
        tokio::spawn(async move {
            bub.kline_data(tx).await;
        });

        while let Some(v) = rx.recv().await {
            println!("Kline data: {:#?}", v.list.last().unwrap());
        }
    }

    #[tokio::test]
    async fn test_time() {
        let api_key = "key".to_string();
        let api_secret = "secret".to_string();
        let exchange = Exchanges::Bybit;
        let bub = Normalized::init(api_key, api_secret, exchange);
        bub.exchange_time().await;
    }

    #[tokio::test]
    async fn test_init() {
        let (tx, mut rx) = mpsc::channel(100);
        tokio::spawn(send_data(tx));

        let received = rx.recv().await.unwrap();
        println!("Got: {}", received);
    }

    async fn send_data(sen: mpsc::Sender<String>) {
        let val = String::from("hi");
        sen.send(val).await.unwrap();
    }
}
