// Declare the ss struct
use std::sync::Arc;
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
    pub clients: BybitClient,
    pub private: BybitPrivate,
    pub markets: Vec<MarketMessage>,
    pub symbols: Vec<&'static str>,
}

type WrappedState = Arc<Mutex<SharedState>>;

impl SharedState {
    pub fn new() -> Self {
        let log = Logger;
        Self {
            logging: log,
            clients: BybitClient::default(),
            private: BybitPrivate::default(),
            markets: Vec::with_capacity(2),
            symbols: ["MATICUSDT", "SKLUSDT"].to_vec(),
        }
    }

    pub fn add_clients(&mut self, key: String, secret: String) {
        self.clients = BybitClient::init(key, secret);
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
    state.lock().await.markets = vec![
        MarketMessage::Bybit(BybitMarket::default()),
        MarketMessage::Binance(BinanceMarket::default()),
    ];
    let bit_ss_sender_clone = state_sender.clone();
    let bybit_state_clone = state.clone();
    let binance_state_clone = state.clone();
    let (bybit_sender, mut bybit_receiver) = mpsc::unbounded_channel::<BybitMarket>();
    let (binance_sender, mut binance_receiver) = mpsc::unbounded_channel::<BinanceMarket>();
    let (private_sender, mut private_receiver) = mpsc::unbounded_channel::<BybitPrivate>();
    let binance_symbols = state.lock().await.symbols.clone();
    let symbols = state.lock().await.symbols.clone();
    let bybit_client = state.lock().await.clients.clone();
    let client_state_clone = state.clone();

    tokio::spawn(async move {
        let subscriber = BybitClient::default();
        let _ = subscriber.market_subscribe(symbols, bybit_sender).await;
    });

    tokio::task::spawn_blocking(move || {
        let subscriber = BinanceClient::default();
        let _ = subscriber.market_subscribe(binance_symbols, binance_sender);
    });

    tokio::spawn(async move {
        let subscriber = bybit_client;
        let _ = subscriber.private_subscribe(private_sender).await;
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
            Some(v) = private_receiver.recv() => {
                let mut state = client_state_clone.lock().await;
                state.private = v;
                state_sender
                    .send(state.clone())
                    .expect("Failed to send state to main thread");
                logger.debug("Private data received");
            }
            else => break,
        }
    }
}
