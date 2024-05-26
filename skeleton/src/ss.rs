// Declare the ss struct
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{
    mpsc::{self, UnboundedReceiver},
    Mutex,
};

use crate::{
    exchanges::{
        ex_binance::{BinanceClient, BinanceMarket},
        ex_bybit::{BybitClient, BybitMarket, BybitPrivate},
        exchange::MarketMessage,
    },
    util::logger::{self, Logger},
};

#[derive(Debug, Clone)]
pub struct SharedState {
    pub logging: Logger,
    pub clients: HashMap<String, BybitClient>,
    pub private: HashMap<String, Arc<Mutex<mpsc::UnboundedReceiver<BybitPrivate>>>>,
    pub markets: Vec<MarketMessage>,
    pub symbols: Vec<&'static str>,
}

type WrappedState = Arc<Mutex<SharedState>>;

impl SharedState {
    pub fn new() -> Self {
        let log = Logger;
        Self {
            logging: log,
            clients: HashMap::new(),
            private: HashMap::new(),
            markets: [MarketMessage::Bybit(BybitMarket::default()), MarketMessage::Binance(BinanceMarket::default())].to_vec(),
            symbols: Vec::new(),
        }
    }

    pub fn add_clients(&mut self, key: String, secret: String, symbol: String) {
        self.clients.insert(symbol, BybitClient::init(key, secret));
    }

    pub fn add_symbols(&mut self, markets: Vec<&'static str>) {
        self.symbols.extend(markets);
    }

    pub fn setup_log(&self, msg: &str) {
        self.logging.info(msg);
    }
}

pub async fn load_data(state: WrappedState, state_sender: mpsc::UnboundedSender<SharedState>) {
    let logger = state.lock().await.logging.clone();
    let bit_ss_sender_clone = state_sender.clone();
    let bybit_state_clone = state.clone();
    let binance_state_clone = state.clone();
    let binance_symbols = state.lock().await.symbols.clone();
    let symbols = state.lock().await.symbols.clone();
    let bybit_clients = state.lock().await.clients.clone();
    let (bybit_sender, mut bybit_receiver) = mpsc::unbounded_channel::<BybitMarket>();
    let (binance_sender, mut binance_receiver) = mpsc::unbounded_channel::<BinanceMarket>();


    for (symbol, client) in bybit_clients {
        let (private_sender, mut private_receiver) = mpsc::unbounded_channel::<BybitPrivate>();
        let _ = &state.lock().await.private.insert(symbol, Arc::new(Mutex::new(private_receiver)));
        tokio::spawn(async move {
            let subscriber = client;
            let _ = subscriber.private_subscribe(private_sender).await;
        });
    }

    tokio::spawn(async move {
        let subscriber = BybitClient::default();
        let _ = subscriber.market_subscribe(symbols, bybit_sender).await;
    });

    tokio::task::spawn_blocking(move || {
        let subscriber = BinanceClient::default();
        let _ = subscriber.market_subscribe(binance_symbols, binance_sender);
    });

    loop {
        tokio::select! {
            Some(v) = bybit_receiver.recv() => {
                let mut state = bybit_state_clone.lock().await;
                state.markets[0] = MarketMessage::Bybit(v);
                bit_ss_sender_clone
                    .send(state.clone())
                    .expect("Failed to send state to main thread");
                logger.debug("Bybit market data received");
            }
            Some(v) = binance_receiver.recv() => {
                let mut state = binance_state_clone.lock().await;
                state.markets[1] = MarketMessage::Binance(v);
                state_sender
                    .send(state.clone())
                    .expect("Failed to send state to main thread");
                logger.debug("Binance market data received");
            }
            else => break,
        }
    }
}
