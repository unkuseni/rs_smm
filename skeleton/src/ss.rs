// Declare the ss struct
use std::fmt::Debug;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{mpsc, Mutex};

use crate::{
    exchanges::{
        ex_binance::{BinanceClient, BinanceMarket},
        ex_bybit::{BybitClient, BybitMarket},
        exchange::{ExchangeClient, MarketMessage, PrivateData},
    },
    util::logger::{self, Logger},
};

#[derive(Debug, Clone)]
pub struct SharedState {
    pub exchange: &'static str,
    pub logging: Logger,
    pub clients: HashMap<String, ExchangeClient>,
    pub private: HashMap<String, Arc<Mutex<mpsc::UnboundedReceiver<PrivateData>>>>,
    pub markets: Vec<MarketMessage>,
    pub symbols: Vec<&'static str>,
}

type WrappedState = Arc<Mutex<SharedState>>;

impl SharedState {
    pub fn new(exchange: &'static str) -> Self {
        let log = Logger;
        Self {
            exchange,
            logging: log,
            clients: HashMap::new(),
            private: HashMap::new(),
            markets: match exchange {
                "bybit" => [MarketMessage::Bybit(BybitMarket::default())].to_vec(),
                "binance" => [MarketMessage::Binance(BinanceMarket::default())].to_vec(),
                "both" => [
                    MarketMessage::Bybit(BybitMarket::default()),
                    MarketMessage::Binance(BinanceMarket::default()),
                ]
                .to_vec(),
                _ => panic!("Invalid exchange"),
            },
            symbols: Vec::new(),
        }
    }

    pub fn add_clients(
        &mut self,
        key: String,
        secret: String,
        symbol: String,
        exchange: Option<&'static str>,
    ) {
        match self.exchange {
            "bybit" => {
                let client = BybitClient::init(key, secret);
                self.clients.insert(symbol, ExchangeClient::Bybit(client));
            }
            "binance" => {
                let client = BinanceClient::init(key, secret);
                self.clients.insert(symbol, ExchangeClient::Binance(client));
            }
            "both" => {
                if let Some(v) = exchange {
                    match v {
                        "bybit" => {
                            let client = BybitClient::init(key, secret);
                            self.clients.insert(symbol, ExchangeClient::Bybit(client));
                        }
                        "binance" => {
                            let client = BinanceClient::init(key, secret);
                            self.clients.insert(symbol, ExchangeClient::Binance(client));
                        }
                        _ => panic!("Invalid exchange"),
                    }
                }
            }
            _ => panic!("Invalid exchange"),
        }
    }

    pub fn add_symbols(&mut self, markets: Vec<&'static str>) {
        self.symbols.extend(markets);
    }

    pub fn setup_log(&self, msg: &str) {
        self.logging.info(msg);
    }
}

pub async fn load_data(state: WrappedState, state_sender: mpsc::UnboundedSender<SharedState>) {
    match state.lock().await.exchange {
        "bybit" => {
            let symbols = state.lock().await.symbols.clone();
            let clients = state.lock().await.clients.clone();
            let (sender, mut receiver) = mpsc::unbounded_channel::<BybitMarket>();

            for (symbol, client) in clients {
                let (private_sender, mut private_receiver) =
                    mpsc::unbounded_channel::<PrivateData>();
                let _ = &state
                    .lock()
                    .await
                    .private
                    .insert(symbol, Arc::new(Mutex::new(private_receiver)));
                tokio::spawn(async move {
                    let subscriber = match client {
                        ExchangeClient::Bybit(client) => client,
                        _ => panic!("Invalid exchange"),
                    };
                    let _ = subscriber.private_subscribe(private_sender).await;
                });
            }

            tokio::spawn(async move {
                let subscriber = BybitClient::default();
                let _ = subscriber.market_subscribe(symbols, sender).await;
            });

            while let Some(v) =  receiver.recv().await {
                    let mut state = state.lock().await;
                state.markets[0] = MarketMessage::Bybit(v);
                state_sender
                    .send(state.clone())
                    .expect("Failed to send state to main thread");
                }
            },
        "binance" => {
            let symbols = state.lock().await.symbols.clone();
            let clients = state.lock().await.clients.clone();
            let (sender, mut receiver) = mpsc::unbounded_channel::<BinanceMarket>();

            for (symbol, client) in clients {
                let (private_sender, mut private_receiver) =
                    mpsc::unbounded_channel::<PrivateData>();
                let _ = &state
                    .lock()
                    .await
                    .private
                    .insert(symbol, Arc::new(Mutex::new(private_receiver)));
                tokio::spawn(async move {
                    let subscriber = match client {
                        ExchangeClient::Binance(client) => client,
                        _ => panic!("Invalid exchange"),
                    };
                    let _ = subscriber.private_subscribe(private_sender);
                });
            }

            tokio::spawn(async move {
                let subscriber = BinanceClient::default();
                let _ = subscriber.market_subscribe(symbols, sender);
            });

            while let Some(v) = receiver.recv().await {
                let mut state = state.lock().await;
                state.markets[0] = MarketMessage::Binance(v);
                state_sender
                    .send(state.clone())
                    .expect("Failed to send state to main thread");
            }
        },
        "both" => {
            let logger = state.lock().await.logging.clone();
            let bit_ss_sender_clone = state_sender.clone();
            let bybit_state_clone = state.clone();
            let binance_state_clone = state.clone();
            let binance_symbols = state.lock().await.symbols.clone();
            let symbols = state.lock().await.symbols.clone();
            let clients = state.lock().await.clients.clone();
            let (bybit_sender, mut bybit_receiver) = mpsc::unbounded_channel::<BybitMarket>();
            let (binance_sender, mut binance_receiver) = mpsc::unbounded_channel::<BinanceMarket>();
            if clients.is_empty() {
                logger.error("No clients found");
            }
            for (symbol, client) in clients {
                let (private_sender, mut private_receiver) =
                    mpsc::unbounded_channel::<PrivateData>();
                let _ = &state
                    .lock()
                    .await
                    .private
                    .insert(symbol, Arc::new(Mutex::new(private_receiver)));
                tokio::spawn(async move {
                    let _ = match client {
                        ExchangeClient::Bybit(client) => {
                            client.private_subscribe(private_sender).await
                        }
                        ExchangeClient::Binance(client) => client.private_subscribe(private_sender),
                    };
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
                        // logger.debug("Bybit market data received");
                    }
                    Some(v) = binance_receiver.recv() => {
                        let mut state = binance_state_clone.lock().await;
                        state.markets[1] = MarketMessage::Binance(v);
                        state_sender
                            .send(state.clone())
                            .expect("Failed to send state to main thread");
                        // logger.debug("Binance market data received");
                    }
                    else => break,
                }
            }
        }
        _ => {
            panic!("Invalid exchange");
        }
    }
}
