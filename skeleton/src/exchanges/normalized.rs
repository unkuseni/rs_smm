use binance::{api::Binance, general::General as binGeneral};
use bybit::{
    api::Bybit,
    general::General,
    market::MarketData,
    model::{Category, KlineRequest, KlineSummary, OrderBookUpdate, Subscription, WebsocketEvents},
    ws::Stream as BybitStream,
};
use tokio::sync::mpsc;

use super::exchange::{Exchange, Exchanges};
#[derive(Clone, Debug)]
pub struct Normalized {
    pub key: String,
    pub secret: String,
    pub exchange: Exchanges,
}

impl Exchange for Normalized {
    fn init(key: String, secret: String, exchange: Exchanges) -> Self {
        Self {
            key,
            secret,
            exchange,
        }
    }
    async fn exchange_time(&self) -> u64 {
        match self.exchange {
            Exchanges::Bybit => {
                let general: General = Bybit::new(None, None);
                let response = general.get_server_time().await;
                if let Ok(v) = response {
                    println!("Server time: {}", v.time_second);
                    v.time_second
                } else {
                    0
                }
            }
            Exchanges::Binance => {
                let response = tokio::task::spawn_blocking(|| {
                    let general: binGeneral = Binance::new(None, None);
                    general.get_server_time()
                })
                .await
                .expect("Failed to get server time");
                if let Ok(v) = response {
                    println!("Server time: {}", v.server_time);
                    v.server_time
                } else {
                    0
                }
            }
        }
    }

    async fn orderbook(&self, symbol: &str, sender: mpsc::Sender<OrderBookUpdate>) {
        let order_book: BybitStream = Bybit::new(None, None);
        let bba = format!("orderbook.1.{}", symbol).to_string();
        let surr = format!("orderbook.50.{}", symbol).to_string();
        let request_args: Vec<&str> = vec![&bba, &surr];

        // Perform WebSocket subscription
        let request = Subscription::new("subscribe", request_args);
        let _stream = order_book
            .ws_subscribe(request, Category::Linear, move |event: WebsocketEvents| {
                if let WebsocketEvents::OrderBookEvent(v) = event {
                    sender.try_send(v).unwrap();
                }
                Ok(())
            })
            .await;
    }

    async fn kline_data(&self, sender: mpsc::Sender<KlineSummary>) {
        let market: MarketData = Bybit::new(None, None);
        let req = KlineRequest::new(
            Some(Category::Linear),
            "MATICUSDT",
            "60",
            None,
            None,
            Some(10),
        );
        loop {
            let response = market.get_klines(req.clone()).await;

            // Send the response through the sender and handle errors
            if let Ok(v) = response {
                if let Err(e) = sender.send(v).await {
                    eprintln!("Error sending data through sender: {}", e);
                } else {
                    println!("Data sent successfully");
                }
            }
        }
    }
}
