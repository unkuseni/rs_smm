// Shared-state loading and distribution.
//
// The loaders own the authoritative order books and trade buffers for every
// symbol. Websocket handlers forward lightweight `MarketEvent` deltas, which
// are applied here before an `Arc<SharedState>` snapshot is sent to the
// market maker. `Arc::make_mut` copy-on-write semantics mean the snapshot is
// only actually cloned while the consumer is still holding the previous one,
// so the steady-state cost per event is a delta application plus a reference
// count bump.

use std::{collections::HashMap, sync::Arc};
use tokio::sync::mpsc;

use crate::exchanges::ex_binance::{BinanceClient, BinanceMarket, BinancePrivate};
use crate::exchanges::ex_bybit::{BybitClient, BybitMarket, BybitPrivate};
use crate::exchanges::exchange::{
    Client, Exchange, MarketEvent, MarketMessage, PrivateData, TaggedPrivate,
};
use crate::util::logger::Logger;
use bybit::{Bybit, Category, InstrumentInfo, InstrumentRequest, MarketData, WsTrade};
use std::collections::VecDeque;

use crate::util::localorderbook::LocalBook;
use binance::api::Binance as BinanceApi;
use binance::futures::general::FuturesGeneral;
use binance::futures::model::Filters;

/// A struct to hold the shared state of the program.
///
/// * `exchange` - "bybit", "binance", or "both".
/// * `logging` - The logger used for events.
/// * `clients` - A mapping of symbols to exchange clients.
/// * `private` - A mapping of symbols to the latest private data.
/// * `markets` - The market messages (one per exchange).
/// * `symbols` - The symbols being traded.
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
    /// Creates a new `SharedState` for the given exchange.
    pub fn new(exchange: String) -> Self {
        let log = Logger;
        Self {
            exchange: exchange.clone(),
            logging: log,
            clients: HashMap::new(),
            private: HashMap::new(),
            markets: match exchange.as_str() {
                "bybit" => vec![MarketMessage::Bybit(BybitMarket::default())],
                "binance" => vec![MarketMessage::Binance(BinanceMarket::default())],
                "both" => vec![
                    MarketMessage::Bybit(BybitMarket::default()),
                    MarketMessage::Binance(BinanceMarket::default()),
                ],
                _ => panic!("Invalid exchange"),
            },
            symbols: Vec::new(),
        }
    }

    /// Adds a client for the given symbol on the configured exchange.
    ///
    /// When the configured exchange is "both", the per-client `exchange`
    /// argument selects which exchange the key belongs to.
    pub fn add_clients(
        &mut self,
        key: String,
        secret: String,
        symbol: String,
        exchange: Option<String>,
    ) {
        match self.exchange.as_str() {
            "bybit" => {
                let client = BybitClient::init(key, secret);
                self.clients.insert(symbol, Client::Bybit(client));
            }
            "binance" => {
                let client = BinanceClient::init(key, secret);
                self.clients.insert(symbol, Client::Binance(client));
            }
            "both" => {
                let Some(v) = exchange else {
                    eprintln!(
                        "No exchange specified for client of {}; client ignored",
                        symbol
                    );
                    return;
                };
                match v.as_str() {
                    "bybit" => {
                        let client = BybitClient::init(key, secret);
                        self.clients.insert(symbol, Client::Bybit(client));
                    }
                    "binance" => {
                        let client = BinanceClient::init(key, secret);
                        self.clients.insert(symbol, Client::Binance(client));
                    }
                    _ => panic!("Invalid exchange"),
                }
            }
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

/// Asynchronously loads data from the configured exchange and sends `Arc`
/// snapshots of the shared state to the market maker.
pub async fn load_data(state: SharedState, state_sender: mpsc::UnboundedSender<Arc<SharedState>>) {
    let exchange = state.exchange.clone();
    match exchange.as_str() {
        "bybit" => load_bybit(state, state_sender).await,
        "binance" => load_binance(state, state_sender).await,
        "both" => load_both(state, state_sender).await,
        _ => {
            panic!("Invalid exchange");
        }
    };
}

/// Fetches Bybit instrument info and populates a fresh `LocalBook` per symbol.
async fn bybit_instrument_books(symbols: &[String]) -> HashMap<String, Arc<LocalBook>> {
    let mut books = HashMap::new();
    for s in symbols {
        books.insert(s.clone(), Arc::new(LocalBook::new()));
    }
    let cl: MarketData = Bybit::new(None, None);
    for s in symbols {
        let req = InstrumentRequest::new(Category::Linear, Some(s), None, None, None, None, None);
        if let Ok(res) = cl.get_instrument_info(req).await {
            // 0.4 returns an untagged enum; linear perps come back as `Futures`.
            let InstrumentInfo::Futures(info) = res.result else {
                continue;
            };
            let Some(inst) = info.list.first() else {
                continue;
            };
            if let Some(book) = books.get_mut(s) {
                let b = Arc::make_mut(book);
                b.tick_size = inst.price_filter.tick_size;
                b.lot_size = inst.lot_size_filter.qty_step.unwrap_or(0.0);
                b.post_only_max = inst
                    .lot_size_filter
                    .post_only_max_order_qty
                    .unwrap_or(inst.lot_size_filter.max_order_qty);
                b.min_order_size = inst.lot_size_filter.min_order_qty;
                b.min_notional = inst.lot_size_filter.min_notional_value.unwrap_or(0.0);
            }
        }
    }
    books
}

/// Fetches Binance symbol info and populates a fresh `LocalBook` per symbol.
async fn binance_instrument_books(symbols: &[String]) -> HashMap<String, Arc<LocalBook>> {
    let mut books = HashMap::new();
    for s in symbols {
        books.insert(s.clone(), Arc::new(LocalBook::new()));
    }
    let cl: FuturesGeneral = BinanceApi::new(None, None);
    for s in symbols {
        let Ok(info) = cl.get_symbol_info(s).await else {
            continue;
        };
        let mut tick = 0.0;
        let mut step = 0.0;
        let mut min_qty = 0.0;
        let mut max_qty = 0.0;
        let mut min_notional = 0.0;
        for f in &info.filters {
            match f {
                Filters::PriceFilter { tick_size, .. } => {
                    tick = tick_size.parse().unwrap_or(0.0);
                }
                Filters::LotSize {
                    min_qty: m,
                    max_qty: mx,
                    step_size: st,
                    ..
                } => {
                    min_qty = m.parse().unwrap_or(0.0);
                    max_qty = mx.parse().unwrap_or(0.0);
                    step = st.parse().unwrap_or(0.0);
                }
                Filters::MinNotional { notional, .. } => {
                    min_notional = notional
                        .as_ref()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0.0);
                }
                _ => {}
            }
        }
        if let Some(book) = books.get_mut(s) {
            let b = Arc::make_mut(book);
            b.tick_size = tick;
            b.lot_size = step;
            b.min_order_size = min_qty;
            b.min_notional = min_notional;
            b.post_only_max = max_qty;
        }
    }
    books
}

fn new_trade_buffers(symbols: &[String]) -> HashMap<String, Arc<VecDeque<WsTrade>>> {
    symbols
        .iter()
        .map(|s| (s.clone(), Arc::new(VecDeque::with_capacity(5000))))
        .collect()
}

/// Asynchronously loads market and private data from Bybit.
async fn load_bybit(state: SharedState, state_sender: mpsc::UnboundedSender<Arc<SharedState>>) {
    let symbols = state.symbols.clone();
    let clients = state.clients.clone();

    // Seed private-data entries before sharing the state.
    let mut state = state;
    for symbol in clients.keys() {
        state
            .private
            .insert(symbol.clone(), PrivateData::Bybit(BybitPrivate::default()));
    }
    let mut shared = Arc::new(state);

    // Authoritative books and trades owned by this loader.
    let mut books = bybit_instrument_books(&symbols).await;
    let mut trades = new_trade_buffers(&symbols);

    // Private subscriptions (one task per client).
    let (private_sender, mut private_receiver) = mpsc::unbounded_channel::<TaggedPrivate>();
    for (symbol, client) in clients {
        let sender_clone = private_sender.clone();
        tokio::spawn(async move {
            let subscriber = match client {
                Client::Bybit(client) => client,
                _ => {
                    eprintln!("Invalid exchange client for {}", symbol);
                    return;
                }
            };
            subscriber.private_subscribe(sender_clone, symbol).await;
        });
    }

    // Public market subscription.
    let (market_sender, mut market_receiver) = mpsc::unbounded_channel::<MarketEvent>();
    tokio::spawn(async move {
        let subscriber = BybitClient::default();
        subscriber.market_subscribe(symbols, market_sender).await;
    });

    loop {
        tokio::select! {
            Some(ev) = market_receiver.recv() => {
                let (symbol, timestamp) = match ev {
                    MarketEvent::Book { symbol, bids, asks, timestamp, bba } => {
                        if let Some(book_arc) = books.get_mut(&symbol) {
                            let book = Arc::make_mut(book_arc);
                            if bba {
                                book.update_bba(bids, asks, timestamp);
                            } else {
                                book.update(bids, asks, timestamp);
                            }
                        }
                        (symbol, timestamp)
                    }
                    MarketEvent::Trade { symbol, trade } => {
                        let ts = trade.timestamp;
                        if let Some(queue) = trades.get_mut(&symbol) {
                            let q = Arc::make_mut(queue);
                            if q.len() == q.capacity() {
                                q.pop_front();
                            }
                            q.push_back(trade);
                        }
                        (symbol, ts)
                    }
                };

                let state = Arc::make_mut(&mut shared);
                let books_msg = books
                    .get(&symbol)
                    .map(|b| vec![(symbol.clone(), b.clone())])
                    .unwrap_or_default();
                let trades_msg = trades
                    .get(&symbol)
                    .map(|t| vec![(symbol.clone(), t.clone())])
                    .unwrap_or_default();
                state.markets[0] = MarketMessage::Bybit(BybitMarket {
                    time: timestamp,
                    books: books_msg,
                    trades: trades_msg,
                });
                if state_sender.send(shared.clone()).is_err() {
                    break;
                }
            }

            Some(data) = private_receiver.recv() => {
                let state = Arc::make_mut(&mut shared);
                state.private.insert(data.symbol, data.data);
                if state_sender.send(shared.clone()).is_err() {
                    break;
                }
            }
        }
    }
}

/// Asynchronously loads market and private data from Binance.
async fn load_binance(state: SharedState, state_sender: mpsc::UnboundedSender<Arc<SharedState>>) {
    let symbols = state.symbols.clone();
    let clients = state.clients.clone();

    let mut state = state;
    for symbol in clients.keys() {
        state.private.insert(
            symbol.clone(),
            PrivateData::Binance(BinancePrivate::default()),
        );
    }
    let mut shared = Arc::new(state);

    let mut books = binance_instrument_books(&symbols).await;
    let mut trades = new_trade_buffers(&symbols);

    let (private_sender, mut private_receiver) = mpsc::unbounded_channel::<TaggedPrivate>();
    for (symbol, client) in clients {
        let sender_clone = private_sender.clone();
        tokio::spawn(async move {
            let subscriber = match client {
                Client::Binance(client) => client,
                _ => {
                    eprintln!("Invalid exchange client for {}", symbol);
                    return;
                }
            };
            subscriber.private_subscribe(sender_clone, symbol).await;
        });
    }

    let (market_sender, mut market_receiver) = mpsc::unbounded_channel::<MarketEvent>();
    tokio::spawn(async move {
        let subscriber = BinanceClient::default();
        subscriber.market_subscribe(symbols, market_sender).await;
    });

    loop {
        tokio::select! {
            Some(ev) = market_receiver.recv() => {
                let (symbol, timestamp) = match ev {
                    MarketEvent::Book { symbol, bids, asks, timestamp, bba } => {
                        if let Some(book_arc) = books.get_mut(&symbol) {
                            let book = Arc::make_mut(book_arc);
                            if bba {
                                book.update_binance_bba(bids, asks, timestamp);
                            } else {
                                book.update(bids, asks, timestamp);
                            }
                        }
                        (symbol, timestamp)
                    }
                    MarketEvent::Trade { symbol, trade } => {
                        let ts = trade.timestamp;
                        if let Some(queue) = trades.get_mut(&symbol) {
                            let q = Arc::make_mut(queue);
                            if q.len() == q.capacity() {
                                q.pop_front();
                            }
                            q.push_back(trade);
                        }
                        (symbol, ts)
                    }
                };

                let state = Arc::make_mut(&mut shared);
                let books_msg = books
                    .get(&symbol)
                    .map(|b| vec![(symbol.clone(), b.clone())])
                    .unwrap_or_default();
                let trades_msg = trades
                    .get(&symbol)
                    .map(|t| vec![(symbol.clone(), t.clone())])
                    .unwrap_or_default();
                state.markets[0] = MarketMessage::Binance(BinanceMarket {
                    time: timestamp,
                    books: books_msg,
                    trades: trades_msg,
                });
                if state_sender.send(shared.clone()).is_err() {
                    break;
                }
            }

            Some(data) = private_receiver.recv() => {
                let state = Arc::make_mut(&mut shared);
                state.private.insert(data.symbol, data.data);
                if state_sender.send(shared.clone()).is_err() {
                    break;
                }
            }
        }
    }
}

/// Asynchronously loads data from both Bybit and Binance. Experimental.
async fn load_both(state: SharedState, state_sender: mpsc::UnboundedSender<Arc<SharedState>>) {
    let logger = state.logging.clone();
    let symbols = state.symbols.clone();
    let clients = state.clients.clone();

    if clients.is_empty() {
        logger.error("No clients found");
        return;
    }

    let mut shared = Arc::new(state);

    let mut bybit_books = bybit_instrument_books(&symbols).await;
    let mut binance_books = binance_instrument_books(&symbols).await;
    let mut bybit_trades = new_trade_buffers(&symbols);
    let mut binance_trades = new_trade_buffers(&symbols);

    let (private_sender, mut private_receiver) = mpsc::unbounded_channel::<TaggedPrivate>();
    for (symbol, client) in clients {
        let sender_clone = private_sender.clone();
        match client {
            Client::Bybit(client) => {
                let state = Arc::make_mut(&mut shared);
                state
                    .private
                    .insert(symbol.clone(), PrivateData::Bybit(BybitPrivate::default()));
                tokio::spawn(async move {
                    client.private_subscribe(sender_clone, symbol).await;
                });
            }
            Client::Binance(client) => {
                let state = Arc::make_mut(&mut shared);
                state.private.insert(
                    symbol.clone(),
                    PrivateData::Binance(BinancePrivate::default()),
                );
                tokio::spawn(async move {
                    client.private_subscribe(sender_clone, symbol).await;
                });
            }
        }
    }

    let (bybit_sender, mut bybit_receiver) = mpsc::unbounded_channel::<MarketEvent>();
    let bybit_symbols = symbols.clone();
    tokio::spawn(async move {
        let subscriber = BybitClient::default();
        subscriber
            .market_subscribe(bybit_symbols, bybit_sender)
            .await;
    });

    let (binance_sender, mut binance_receiver) = mpsc::unbounded_channel::<MarketEvent>();
    let binance_symbols = symbols.clone();
    tokio::spawn(async move {
        let subscriber = BinanceClient::default();
        subscriber
            .market_subscribe(binance_symbols, binance_sender)
            .await;
    });

    loop {
        tokio::select! {
            Some(ev) = bybit_receiver.recv() => {
                let (symbol, timestamp) = match ev {
                    MarketEvent::Book { symbol, bids, asks, timestamp, bba } => {
                        if let Some(book_arc) = bybit_books.get_mut(&symbol) {
                            let book = Arc::make_mut(book_arc);
                            if bba { book.update_bba(bids, asks, timestamp); } else { book.update(bids, asks, timestamp); }
                        }
                        (symbol, timestamp)
                    }
                    MarketEvent::Trade { symbol, trade } => {
                        let ts = trade.timestamp;
                        if let Some(queue) = bybit_trades.get_mut(&symbol) {
                            let q = Arc::make_mut(queue);
                            if q.len() == q.capacity() { q.pop_front(); }
                            q.push_back(trade);
                        }
                        (symbol, ts)
                    }
                };
                let state = Arc::make_mut(&mut shared);
                let books_msg = bybit_books
                    .get(&symbol)
                    .map(|b| vec![(symbol.clone(), b.clone())])
                    .unwrap_or_default();
                let trades_msg = bybit_trades
                    .get(&symbol)
                    .map(|t| vec![(symbol.clone(), t.clone())])
                    .unwrap_or_default();
                state.markets[0] = MarketMessage::Bybit(BybitMarket {
                    time: timestamp,
                    books: books_msg,
                    trades: trades_msg,
                });
                if state_sender.send(shared.clone()).is_err() {
                    break;
                }
            }

            Some(ev) = binance_receiver.recv() => {
                let (symbol, timestamp) = match ev {
                    MarketEvent::Book { symbol, bids, asks, timestamp, bba } => {
                        if let Some(book_arc) = binance_books.get_mut(&symbol) {
                            let book = Arc::make_mut(book_arc);
                            if bba { book.update_binance_bba(bids, asks, timestamp); } else { book.update(bids, asks, timestamp); }
                        }
                        (symbol, timestamp)
                    }
                    MarketEvent::Trade { symbol, trade } => {
                        let ts = trade.timestamp;
                        if let Some(queue) = binance_trades.get_mut(&symbol) {
                            let q = Arc::make_mut(queue);
                            if q.len() == q.capacity() { q.pop_front(); }
                            q.push_back(trade);
                        }
                        (symbol, ts)
                    }
                };
                let state = Arc::make_mut(&mut shared);
                let books_msg = binance_books
                    .get(&symbol)
                    .map(|b| vec![(symbol.clone(), b.clone())])
                    .unwrap_or_default();
                let trades_msg = binance_trades
                    .get(&symbol)
                    .map(|t| vec![(symbol.clone(), t.clone())])
                    .unwrap_or_default();
                state.markets[1] = MarketMessage::Binance(BinanceMarket {
                    time: timestamp,
                    books: books_msg,
                    trades: trades_msg,
                });
                if state_sender.send(shared.clone()).is_err() {
                    break;
                }
            }

            Some(data) = private_receiver.recv() => {
                let state = Arc::make_mut(&mut shared);
                state.private.insert(data.symbol, data.data);
                if state_sender.send(shared.clone()).is_err() {
                    break;
                }
            }
            else => break,
        }
    }
}
