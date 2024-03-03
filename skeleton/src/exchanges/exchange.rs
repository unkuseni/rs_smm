use bybit::model::{KlineSummary, OrderBookUpdate};
use std::future::Future;
use tokio::sync::mpsc;

pub trait Exchange {
    fn init(key: String, secret: String, exchange: Exchanges) -> Self;
    fn orderbook(&self, symbol: &str, sender: mpsc::Sender<OrderBookUpdate>) -> impl Future<Output = ()>;
    fn exchange_time(&self) -> impl Future<Output = u64>;
    fn kline_data(&self, sender: mpsc::Sender<KlineSummary>) -> impl Future<Output = ()>;
}

#[derive(Clone, Debug)]
pub enum Exchanges {
    Bybit,
    Binance,
}
