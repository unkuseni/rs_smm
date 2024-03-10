use std::collections::HashSet;

use tokio::sync::mpsc;

use crate::{
    exchanges::{
        ex_binance::BinanceClient,
        ex_bybit::{BybitClient, BybitMarket, BybitPrivate},
        exchange::{Exchange},
    },
    util::logger::Logger,
};

pub struct SharedState {
    pub logging: Logger,
    pub markets: BybitMarket,
    pub private: BybitPrivate,
    pub clients: Vec<Exchange>,
    pub symbols: Vec<&'static str>,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            logging: Logger,
            markets: BybitMarket::default(),
            private: BybitPrivate::default(),
            clients: Vec::new(),
            symbols: Vec::new(),
        }
    }
}

impl SharedState {
    pub async fn load_markets(&mut self, symbols: Vec<&'static str>) {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let client = BybitClient::init("key".to_string(), "secret".to_string());

        let stream = tokio::spawn(async move {
            client.market_subscribe(symbols, tx).await;
        });
        while let Some(v) = rx.recv().await {
            self.markets = v;
        }
    }

    pub async fn load_clients(&mut self, clients: Vec<String>) -> Vec<Exchange> {
        let mut client_arr = Vec::new();
        for v in clients {
            match v.as_str() {
                "bybit" => {
                    let client = BybitClient::init("key".to_string(), "secret".to_string());
                    client_arr.push(Exchange::Bybit(client));
                }
                "binance" => {
                    let client = BinanceClient::init("key".to_string(), "secret".to_string());
                    client_arr.push(Exchange::Binance(client));
                }
                _ => {
                    println!("Unknown client: {}", v);
                }
            }
        }
        client_arr
    }
}
