// Declare the ss struct
use std::fmt::Debug;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{mpsc, Mutex};

use crate::exchanges::ex_binance::BinancePrivate;
use crate::exchanges::ex_bybit::BybitPrivate;
use crate::exchanges::exchange::TaggedPrivate;
use crate::{
    exchanges::{
        ex_binance::{BinanceClient, BinanceMarket},
        ex_bybit::{BybitClient, BybitMarket},
        exchange::{Client, Exchange, MarketMessage, PrivateData},
    },
    util::logger::Logger,
};

#[derive(Debug, Clone)]
pub struct SharedState {
    pub exchange: String,
    pub logging: Logger,
    pub clients: HashMap<String, Client>,
    pub private: HashMap<String, PrivateData>,
    pub markets: Vec<MarketMessage>,
    pub symbols: Vec<String>,
}

impl SharedState {
    /// Creates a new instance of `SharedState`.
    ///
    /// # Arguments
    ///
    /// * `exchange` - The exchange where the market is traded. Can be "bybit", "binance", or "both".
    ///
    /// # Returns
    ///
    /// A new instance of `SharedState` with default values.
    pub fn new(exchange: String) -> Self {
        // Create a new logger
        let log = Logger;

        // Initialize the `SharedState` struct with default values
        Self {
            exchange: exchange.clone(), // The exchange where the market is traded
            logging: log,               // The logger for the application
            clients: HashMap::new(),    // A hashmap to store exchange clients
            private: HashMap::new(),    // A hashmap to store private data
            markets: match exchange.as_str() {
                "bybit" => {
                    // If the exchange is "bybit", initialize the `markets` vector with a Bybit market
                    vec![MarketMessage::Bybit(BybitMarket::default())]
                }
                "binance" => {
                    // If the exchange is "binance", initialize the `markets` vector with a Binance market
                    vec![MarketMessage::Binance(BinanceMarket::default())]
                }
                "both" => {
                    // If the exchange is "both", initialize the `markets` vector with both a Bybit and Binance market
                    vec![
                        MarketMessage::Bybit(BybitMarket::default()),
                        MarketMessage::Binance(BinanceMarket::default()),
                    ]
                }
                _ => panic!("Invalid exchange"), // Panic if the exchange is not valid
            },
            symbols: Vec::new(), // A vector to store symbols of markets
        }
    }

    /// Adds clients to the `SharedState` struct.
    ///
    /// # Arguments
    ///
    /// * `key` - The API key used for authentication.
    /// * `secret` - The API secret used for authentication.
    /// * `symbol` - The symbol of the market.
    /// * `exchange` - The exchange where the market is traded.
    ///
    /// # Panics
    ///
    /// If the `exchange` is not "bybit", "binance", or "both".
    pub fn add_clients(
        &mut self,
        key: String,
        secret: String,
        symbol: String,
        exchange: Option<String>,
    ) {
        // Check the exchange and add the corresponding client.
        match self.exchange.as_str() {
            // If the exchange is "bybit", add a BybitClient.
            "bybit" => {
                let client = BybitClient::init(key, secret);
                self.clients.insert(symbol, Client::Bybit(client));
            }
            // If the exchange is "binance", add a BinanceClient.
            "binance" => {
                let client = BinanceClient::init(key, secret);
                self.clients.insert(symbol, Client::Binance(client));
            }
            // If the exchange is "both", check the `exchange` argument and add the corresponding client.
            "both" => {
                if let Some(v) = exchange {
                    match v.as_str() {
                        // If the `exchange` is "bybit", add a BybitClient.
                        "bybit" => {
                            let client = BybitClient::init(key, secret);
                            self.clients.insert(symbol, Client::Bybit(client));
                        }
                        // If the `exchange` is "binance", add a BinanceClient.
                        "binance" => {
                            let client = BinanceClient::init(key, secret);
                            self.clients.insert(symbol, Client::Binance(client));
                        }
                        // If the `exchange` is neither "bybit" nor "binance", panic.
                        _ => panic!("Invalid exchange"),
                    }
                }
            }
            // If the exchange is neither "bybit", "binance", nor "both", panic.
            _ => panic!("Invalid exchange"),
        }
    }

    pub fn add_symbols(&mut self, markets: Vec<String>) {
        self.symbols.extend(markets);
    }

    pub fn setup_log(&self, msg: &str) {
        self.logging.info(msg);
    }
}

/// Asynchronously loads data from the shared state and sends it to the main thread using an unbounded
/// sender.
///
/// # Arguments
///
/// * `state` - The shared state containing the market data.
/// * `state_sender` - The unbounded sender used to send updated state to the main thread.
///
/// # Returns
///
/// This function does not return anything.
///
/// # Panics
///
/// If an invalid exchange is provided, this function will panic.
pub async fn load_data(state: SharedState, state_sender: mpsc::UnboundedSender<SharedState>) {
    let exchange = state.exchange.clone();
    match exchange.as_str() {
        "bybit" => load_bybit(state.clone(), state_sender).await,
        "binance" => load_binance(state.clone(), state_sender).await,
        "both" => load_both(state.clone(), state_sender).await,
        _ => {
            panic!("Invalid exchange");
        }
    };
}

/// Asynchronously loads data from the Binance exchange.
///
/// # Arguments
///
/// * `state` - The shared state containing the market data.
/// * `state_sender` - The unbounded sender used to send updated state to the main thread.
///
/// This function creates an Arc and Mutex to allow safe concurrent access to the shared state.
/// It then clones the symbols and clients from the shared state.
///
/// It creates an unbounded channel to receive market data and iterates over the clients,
/// starting the private subscription for each symbol. The private receiver is inserted into
/// the shared state.
///
/// A blocking task is spawned to handle the market subscription. A loop is used to receive market
/// data from both exchanges.
///
/// When market data is received, it is updated in the shared state and sent to the main thread.
/// When private data is received, it is inserted into the shared state and sent to the main thread.
async fn load_binance(state: SharedState, state_sender: mpsc::UnboundedSender<SharedState>) {
    // Create an Arc and Mutex to allow safe concurrent access to the shared state
    let state = Arc::new(Mutex::new(state));

    // Clone the symbols and clients from the shared state
    let symbols = state.lock().await.symbols.clone();
    let clients = state.lock().await.clients.clone();

    // Create an unbounded channel to receive market data
    let (sender, mut receiver) = mpsc::unbounded_channel::<BinanceMarket>();

    // Iterate over the clients and start the private subscription for each symbol
    let (private_sender, mut private_receiver) = mpsc::unbounded_channel::<TaggedPrivate>();
    for (symbol, client) in clients {
        let sender_clone = private_sender.clone();
        // Insert the private receiver into the shared state
        let _ = &state.lock().await.private.insert(
            symbol.clone(),
            PrivateData::Binance(BinancePrivate::default()),
        );

        // Spawn a blocking task to handle the private subscription
        tokio::task::spawn_blocking(move || {
            // Match the client to a Binance client and start the private subscription
            let subscriber = match client {
                Client::Binance(client) => client,
                _ => panic!("Invalid exchange"),
            };

            let _ = subscriber.private_subscribe(sender_clone, symbol);
        });
    }

    // Spawn a blocking task to handle the market subscription
    tokio::task::spawn_blocking(move || {
        // Create a new BinanceClient instance
        let subscriber = BinanceClient::default();

        // Subscribe to the specified symbols and send the received data to the sender channel

        let _ = subscriber.market_subscribe(symbols, sender);
    });

    // Process the received market data and update the shared state
    // Loop to receive market data from both exchanges.
    loop {
        tokio::select! {
                // Receive Binance market data.
                Some(v) = receiver.recv() => {
            let mut state = state.lock().await;
            // Update the market data in the shared state
            state.markets[0] = MarketMessage::Binance(v);

            // Send the updated state to the main thread
            state_sender
                .send(state.clone())
                .expect("Failed to send state to main thread");
        }

        Some(data) = private_receiver.recv() => {
            let mut state = state.lock().await;
            let key = data.symbol;
            state.private.insert(key, data.data);

            // Send the updated state to the main thread
            state_sender
                .send(state.clone())
                .expect("Failed to send state to main thread");
        }
        }
    }
}

/// Asynchronously loads data from the Bybit exchange.
///
/// # Arguments
///
/// * `state` - The shared state containing the market data.
/// * `state_sender` - The unbounded sender used to send updated state to the main thread.
///
/// This function creates an Arc and Mutex to allow safe concurrent access to the shared state.
/// It then clones the symbols and clients from the shared state.
/// It creates an unbounded channel to receive market data.
/// It iterates over the clients and starts the private subscription for each symbol.
/// It spawns a blocking task to handle the private subscription.
/// It spawns a blocking task to handle the market subscription.
/// Finally, it enters a loop to receive market data and update the shared state.
async fn load_bybit(state: SharedState, state_sender: mpsc::UnboundedSender<SharedState>) {
    // Create an Arc and Mutex to allow safe concurrent access to the shared state
    let state = Arc::new(Mutex::new(state));

    // Clone the symbols and clients from the shared state
    let symbols = state.lock().await.symbols.clone();
    let clients = state.lock().await.clients.clone();

    // Create an unbounded channel to receive market data
    let (sender, mut receiver) = mpsc::unbounded_channel::<BybitMarket>();

    // Iterate over the clients and start the private subscription for each symbol
    let (private_sender, mut private_receiver) = mpsc::unbounded_channel::<TaggedPrivate>();
    for (symbol, client) in clients {
        let sender_clone = private_sender.clone();
        // Insert the private receiver into the shared state
        let _ = &state
            .lock()
            .await
            .private
            .insert(symbol.clone(), PrivateData::Bybit(BybitPrivate::default()));

        // Spawn a blocking task to handle the private subscription
        tokio::spawn(async move {
            // Match the client to a Bybit client and start the private subscription
            let subscriber = match client {
                Client::Bybit(client) => client,
                _ => panic!("Invalid exchange"),
            };

            let _ = subscriber.private_subscribe(sender_clone, symbol).await;
        });
    }

    // Spawn a blocking task to handle the market subscription
    tokio::spawn(async move {
        // Create a new Bybit client and start the market subscription
        let subscriber = BybitClient::default();

        let _ = subscriber.market_subscribe(symbols, sender).await;
    });

    // Process the received market data and update the shared state
    // Loop to receive market data from both exchanges.
    loop {
        tokio::select! {
            // Receive Bybit market data.
            Some(v) = receiver.recv() => {
                let mut state = state.lock().await;
                // Update the market data in the shared state
                state.markets[0] = MarketMessage::Bybit(v);

                // Send the updated state to the main thread
                state_sender
                    .send(state.clone())
                    .expect("Failed to send state to main thread");
            }

            Some(data) = private_receiver.recv() => {
                let mut state = state.lock().await;
                let key = data.symbol;
                state.private.insert(key, data.data);

                // Send the updated state to the main thread
                state_sender
                    .send(state.clone())
                    .expect("Failed to send state to main thread");
            }
        }
    }
}

/// Asynchronously loads data from both Bybit and Binance exchanges.
///
/// # Arguments
///
/// * `state` - The shared state containing the market data.
/// * `state_sender` - The unbounded sender used to send updated state to the main thread.
async fn load_both(state: SharedState, state_sender: mpsc::UnboundedSender<SharedState>) {
    // Clone the state to allow for multiple mutable borrows.
    let state = Arc::new(Mutex::new(state));

    // Get a reference to the logging object.
    let logger = state.lock().await.logging.clone();

    // Clone the state sender for use in the Bybit and Binance spawned tasks.
    let bit_ss_sender_clone = state_sender.clone();

    // Clone the state for use in the Bybit and Binance tasks.
    let bybit_state_clone = state.clone();
    let binance_state_clone = state.clone();

    // Clone the symbols for use in the Bybit and Binance tasks.
    let binance_symbols = state.lock().await.symbols.clone();
    let symbols = state.lock().await.symbols.clone();

    // Clone the clients for use in the Bybit and Binance tasks.
    let clients = state.lock().await.clients.clone();

    // Create unbounded channels for receiving Bybit and Binance market data.
    let (bybit_sender, mut bybit_receiver) = mpsc::unbounded_channel::<BybitMarket>();
    let (binance_sender, mut binance_receiver) = mpsc::unbounded_channel::<BinanceMarket>();

    // Check if there are no clients.
    if clients.is_empty() {
        logger.error("No clients found");
    }

    // Spawn tasks for each client.
    let (private_sender, mut private_receiver) = mpsc::unbounded_channel::<TaggedPrivate>();
    for (symbol, client) in clients {
        let sender_clone = private_sender.clone();

        // Insert the private receiver into the state.
        match client {
            Client::Bybit(client) => {
                // Insert the private receiver for Bybit into the state.
                let _ = &state
                    .lock()
                    .await
                    .private
                    .insert(symbol.clone(), PrivateData::Bybit(BybitPrivate::default()));

                // Spawn a task for Bybit private subscription.
                tokio::spawn(async move {
                    client.private_subscribe(sender_clone, symbol).await;
                });
            }
            Client::Binance(client) => {
                // Insert the private receiver for Binance into the state.
                let _ = &state.lock().await.private.insert(
                    symbol.clone(),
                    PrivateData::Binance(BinancePrivate::default()),
                );

                // Spawn a blocking task for Binance private subscription.
                tokio::task::spawn_blocking(move || {
                    client.private_subscribe(sender_clone, symbol);
                });
            }
        }
    }

    // Spawn a task to subscribe to Bybit market data.
    tokio::spawn(async move {
        let subscriber = BybitClient::default();
        let _ = subscriber.market_subscribe(symbols, bybit_sender).await;
    });

    // Spawn a blocking task to subscribe to Binance market data.
    tokio::task::spawn_blocking(move || {
        let subscriber = BinanceClient::default();
        let _ = subscriber.market_subscribe(binance_symbols, binance_sender);
    });

    // Loop to receive market data from both exchanges.
    loop {
        tokio::select! {
            // Receive Bybit market data.
            Some(v) = bybit_receiver.recv() => {
                let mut state = bybit_state_clone.lock().await;
                state.markets[0] = MarketMessage::Bybit(v);
                bit_ss_sender_clone
                    .send(state.clone())
                    .expect("Failed to send state to main thread");
            }
            // Receive Binance market data.
            Some(v) = binance_receiver.recv() => {
                let mut state = binance_state_clone.lock().await;
                state.markets[1] = MarketMessage::Binance(v);
                state_sender
                    .send(state.clone())
                    .expect("Failed to send state to main thread");
            }

            // Receive private data.
            Some(data) = private_receiver.recv() => {
                let mut state = state.lock().await;
                let key = data.symbol;
                state.private.insert(key, data.data);
                state_sender
                    .send(state.clone())
                    .expect("Failed to send state to main thread");
            }
            // Exit the loop if both channels are closed.
            else => break,
        }
    }
}
