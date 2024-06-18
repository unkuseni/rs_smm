use std::collections::VecDeque;

use binance::futures::account::FuturesAccount;
use bybit::trade::Trader;
use skeleton::exchanges::exchange::{ExchangeClient, PrivateData};
use tokio::sync::mpsc::UnboundedReceiver;

type BybitTrader = Trader;
type BinanceTrader = FuturesAccount;

pub struct QuoteGenerator {
    client: OrderManagement,
    positions: Vec<VecDeque<(String, f64)>>,
    live_buys: VecDeque<(String, f64)>,
    live_sells: VecDeque<(String, f64)>,
}

impl QuoteGenerator {
    pub fn new(mut self, client: ExchangeClient) {
        let mut trader;
        match client {
            ExchangeClient::Bybit(cl) => {
                trader = OrderManagement::Bybit(cl.bybit_trader());
            }
            ExchangeClient::Binance(cl) => {
                trader = OrderManagement::Binance(cl.binance_trader());
            }
        }
        self.client = trader;
    }

    pub fn start_loop(&mut self, mut receiver: UnboundedReceiver<PrivateData>) {}
}

enum OrderManagement {
    Bybit(BybitTrader),
    Binance(BinanceTrader),
}
