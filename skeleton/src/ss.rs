// Declare the ss struct
use std::{collections::HashMap as Hashmap, sync::Arc};
use tokio::sync::{mpsc, Mutex};

use crate::{
    exchanges::ex_bybit::{BybitClient, BybitMarket, BybitPrivate},
    util::logger::Logger,
};

#[derive(Debug, Clone)]
pub struct SharedState {
    pub logging: Logger,
    pub clients: Hashmap<String, BybitClient>,
    pub bybit_market: BybitMarket,
    pub symbols: Vec<&'static str>,
}

impl SharedState {
    pub fn new() -> Self {
        let log = Logger;
        Self {
            logging: log,
            clients: Hashmap::new(),
            bybit_market: BybitMarket::default(),
            symbols: Vec::new(),
        }
    }

    pub fn add_clients(&mut self, clients: Vec<(String, BybitClient)>) {
        for v in clients {
            self.clients.insert(v.0, v.1);
        }
    }

    pub fn add_symbols(&mut self, markets: Vec<&'static str>) {
        self.symbols.extend(markets);
    }

    pub async fn load_data(&mut self, sender: mpsc::) {
        self.logging
            .info("Shared state has been loaded: successfully");
        let (bybit_sender, mut bybit_receiver) = mpsc::unbounded_channel::<BybitMarket>();
        let bybit_symbols = self.symbols.clone();

        tokio::spawn(async move {
            let market_client = BybitClient::default();
            market_client
                .market_subscribe(bybit_symbols, bybit_sender)
                .await;
        });
        let market_data = Arc::new(Mutex::new(self.bybit_market.clone()));
        let m_clone = Arc::clone(&market_data);
        tokio::spawn(async move {
            while let Some(v) = bybit_receiver.recv().await {
                let mut market_data = m_clone.lock().await;
                *market_data = v;
            }
        });
    }

    pub fn setup_log(&self, msg: &str) {
        let new_msg = String::from("Shared state has been setup: successfully");
        self.logging.info(msg);
    }
}
