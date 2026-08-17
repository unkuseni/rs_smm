use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use binance::api::Binance;
use binance::config::Config;
use binance::futures::account::FuturesAccount;
use binance::futures::model::{OrderTradeEvent, OrderUpdate};
use binance::futures::userstream::FuturesUserStream;
use binance::futures::websockets::FuturesStream;
use binance::futures::{FuturesMarket, FuturesWebSockets, FuturesWebsocketEvent};
use binance::general::General;
use binance::model::{AccountUpdateEvent, DepthOrderBookEvent, EventBalance, EventPosition};
use bybit::{Ask, Bid, Category, FastExecData, WsTrade};
use tokio::sync::mpsc;

use crate::util::localorderbook::LocalBook;

use super::exchange::{Exchange, MarketEvent, PrivateData, ProcessTrade, TaggedPrivate};

/// A market snapshot for one symbol, referencing the loader's authoritative
/// book and trade buffers through `Arc` so sending a message is a cheap
/// reference-count bump rather than a full clone.
#[derive(Clone, Debug, Default)]
pub struct BinanceMarket {
    pub time: u64,
    pub books: Vec<(String, Arc<LocalBook>)>,
    pub trades: Vec<(String, Arc<VecDeque<WsTrade>>)>,
}

#[derive(Clone, Debug)]
pub struct BinancePrivate {
    pub time: u64,
    pub wallet: VecDeque<EventBalance>,
    pub orders: HashMap<u64, OrderUpdate>,
    pub positions: VecDeque<EventPosition>,
    pub executions: HashMap<u64, OrderUpdate>,
}

impl Default for BinancePrivate {
    fn default() -> Self {
        Self {
            time: 0,
            wallet: VecDeque::with_capacity(20),
            orders: HashMap::with_capacity(2000),
            positions: VecDeque::with_capacity(500),
            executions: HashMap::with_capacity(2000),
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct BinanceClient {
    pub key: String,
    pub secret: String,
}

// Redact credentials so `{:?}` of any struct holding a client never leaks keys.
impl fmt::Debug for BinanceClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BinanceClient")
            .field("key", &"<redacted>")
            .field("secret", &"<redacted>")
            .finish()
    }
}

impl Exchange for BinanceClient {
    type Quoter = FuturesAccount;

    fn default() -> Self {
        Self {
            key: "".into(),
            secret: "".into(),
        }
    }

    fn init<T>(key: T, secret: T) -> Self
    where
        T: Into<String>,
    {
        Self {
            key: key.into(),
            secret: secret.into(),
        }
    }

    async fn time(&self) -> u64 {
        let general: General = Binance::new(None, None);
        general
            .get_server_time()
            .await
            .map(|v| v.server_time)
            .unwrap_or(0)
    }

    async fn fees(&self) -> f64 {
        let client: FuturesAccount =
            Binance::new(Some(self.key.clone()), Some(self.secret.clone()));
        client
            .account_information()
            .await
            .map(|v| v.fee_tier)
            .unwrap_or(0.0)
    }

    async fn set_leverage(&self, symbol: &str, leverage: u16) -> Result<String, String> {
        let client: FuturesAccount =
            Binance::new(Some(self.key.clone()), Some(self.secret.clone()));
        let leverage = leverage.clamp(1, 100) as u8;
        match client.change_initial_leverage(symbol, leverage).await {
            Ok(_) => Ok(String::from("YES")),
            Err(e) => Err(format!("Failed to set leverage: {}", e)),
        }
    }

    fn trader(&self) -> Self::Quoter {
        let config = Config::default().set_recv_window(2500);
        Binance::new_with_config(
            Some(self.key.to_string()),
            Some(self.secret.to_string()),
            &config,
        )
    }
}

impl BinanceClient {
    /// Subscribes to public market streams for the given symbols and forwards
    /// lightweight deltas (`MarketEvent`) to the sender.
    pub async fn market_subscribe(
        &self,
        symbol: Vec<String>,
        sender: mpsc::UnboundedSender<MarketEvent>,
    ) {
        let request = bin_build_requests(&symbol);
        let stream = FuturesStream::default();
        let mut delay = 600_u64;

        loop {
            let sender = sender.clone();
            // The handler signature is fixed by the binance crate and returns
            // a large error type; boxing it here is not possible.
            #[allow(clippy::result_large_err)]
            let handler = move |event| {
                match event {
                    FuturesWebsocketEvent::DepthOrderBook(DepthOrderBookEvent {
                        symbol,
                        event_time,
                        bids,
                        asks,
                        ..
                    }) => {
                        let new_bids: Vec<Bid> = bids
                            .into_iter()
                            .map(|bid| Bid {
                                price: bid.price,
                                qty: bid.qty,
                            })
                            .collect();
                        let new_asks: Vec<Ask> = asks
                            .into_iter()
                            .map(|ask| Ask {
                                price: ask.price,
                                qty: ask.qty,
                            })
                            .collect();
                        // Treat the small equal-sided snapshots as best-bid/ask updates.
                        let bba = new_bids.len() == new_asks.len()
                            && matches!(new_bids.len(), 5 | 10 | 20);
                        let _ = sender.send(MarketEvent::Book {
                            symbol,
                            bids: new_bids,
                            asks: new_asks,
                            timestamp: event_time,
                            bba,
                        });
                    }
                    FuturesWebsocketEvent::AggrTrades(agg) => {
                        let trade = agg.process_trade();
                        let sym = trade.symbol.clone();
                        let _ = sender.send(MarketEvent::Trade { symbol: sym, trade });
                    }
                    _ => {}
                }
                Ok(())
            };

            match stream.ws_subscribe_multiple(&request, handler).await {
                Ok(_) => {
                    delay = 600;
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    delay = (delay * 2).min(60_000);
                }
            }
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
    }

    /// Subscribes to the user data stream (account, order, and position
    /// updates) for one symbol's API key. The listen key is refreshed on
    /// every (re)connection and kept alive in the background.
    pub async fn private_subscribe(
        &self,
        sender: mpsc::UnboundedSender<TaggedPrivate>,
        symbol: String,
    ) {
        let user_stream: FuturesUserStream = Binance::new(Some(self.key.to_string()), None);

        let mut private_data = BinancePrivate::default();
        let mut orders_keys: VecDeque<u64> = VecDeque::new();
        let mut executions_keys: VecDeque<u64> = VecDeque::new();
        // The handler signature is fixed by the binance crate and returns a
        // large error type; boxing it here is not possible.
        #[allow(clippy::result_large_err)]
        let handler = move |event: FuturesWebsocketEvent| {
            match event {
                FuturesWebsocketEvent::AccountUpdate(AccountUpdateEvent {
                    event_time,
                    data,
                    ..
                }) => {
                    private_data.time = event_time;
                    if private_data.wallet.len() == private_data.wallet.capacity()
                        || (private_data.wallet.capacity() - private_data.wallet.len()) <= 5
                    {
                        for _ in 0..10 {
                            private_data.wallet.pop_front();
                        }
                    }
                    if private_data.positions.len() == private_data.positions.capacity()
                        || (private_data.positions.capacity() - private_data.positions.len())
                            <= data.positions.len()
                    {
                        for _ in 0..(data
                            .positions
                            .len()
                            .saturating_sub(private_data.positions.len()))
                        {
                            private_data.positions.pop_front();
                        }
                    }
                    private_data.positions.extend(data.positions);
                    private_data.wallet.extend(data.balances)
                }
                FuturesWebsocketEvent::OrderTrade(OrderTradeEvent { order, .. }) => {
                    let id_to_find = order.order_id;
                    if order.execution_type == "NEW" || order.order_status == "NEW" {
                        remove_oldest_if_needed(&mut private_data.orders, &mut orders_keys, 2000);
                        private_data.orders.insert(id_to_find, order);
                        orders_keys.push_back(id_to_find);
                    } else if order.execution_type == "TRADE"
                        || order.order_status == "FILLED"
                        || order.order_status == "PARTIALLY_FILLED"
                    {
                        if private_data.orders.remove(&id_to_find).is_some() {
                            orders_keys.retain(|&k| k != id_to_find);
                            remove_oldest_if_needed(
                                &mut private_data.executions,
                                &mut executions_keys,
                                2000,
                            );
                            private_data.executions.insert(id_to_find, order);
                            executions_keys.push_back(id_to_find);
                        }
                    } else if private_data.executions.contains_key(&id_to_find) {
                        remove_oldest_if_needed(
                            &mut private_data.executions,
                            &mut executions_keys,
                            2000,
                        );
                        private_data.executions.insert(id_to_find, order);
                    }
                }
                _ => (),
            };
            let tagged_data =
                TaggedPrivate::new(symbol.clone(), PrivateData::Binance(private_data.clone()));
            let _ = sender.send(tagged_data);
            Ok(())
        };

        let mut delay = 600_u64;
        // The listen key expires after ~60 minutes, so re-request it on every
        // (re)connection and keep it alive in the background.
        loop {
            match user_stream.start().await {
                Ok(answer) => {
                    let listen_key = answer.listen_key.clone();

                    // Background keep-alive: PUT /fapi/v1/listenKey every 50 minutes.
                    let keep_alive_stream = user_stream.clone();
                    let keep_alive_key = listen_key.clone();
                    tokio::spawn(async move {
                        let mut interval = tokio::time::interval(Duration::from_secs(50 * 60));
                        loop {
                            interval.tick().await;
                            if let Err(e) = keep_alive_stream.keep_alive(&keep_alive_key).await {
                                eprintln!("Failed to keep user stream alive: {}", e);
                            }
                        }
                    });

                    let mut web_socket = FuturesWebSockets::new(handler.clone());
                    if let Err(e) = web_socket.connect(&FuturesMarket::USDM, &listen_key).await {
                        eprintln!("Error: {}", e);
                        delay = (delay * 2).min(60_000);
                    } else {
                        let running = AtomicBool::new(true);
                        match web_socket.event_loop(&running).await {
                            Ok(_) => delay = 600,
                            Err(e) => {
                                eprintln!("Error: {}", e);
                                delay = (delay * 2).min(60_000);
                            }
                        }
                    }
                }
                Err(_) => {
                    eprintln!("Not able to start a user stream (Check your API_KEY)");
                    delay = (delay * 2).min(60_000);
                }
            }
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
    }
}

fn bin_build_requests(symbol: &[String]) -> Vec<String> {
    let mut request_args = vec![];

    // Agg Trades request
    let trade_req: Vec<String> = symbol
        .iter()
        .map(|sub| sub.to_lowercase())
        .map(|sub| format!("{}@aggTrade", sub))
        .collect();
    request_args.extend(trade_req);
    let best_book: Vec<String> = symbol
        .iter()
        .map(|sub| sub.to_lowercase())
        .flat_map(|sym| vec![("5", sym.clone()), ("10", sym.clone()), ("20", sym.clone())])
        .map(|(depth, sub)| format!("{}@depth{}@100ms", sub, depth))
        .collect();
    request_args.extend(best_book);
    let book: Vec<String> = symbol
        .iter()
        .map(|sub| sub.to_lowercase())
        .map(|sub| format!("{}@depth@100ms", sub))
        .collect();
    request_args.extend(book);
    request_args
}

pub fn remove_oldest_if_needed<T>(
    map: &mut HashMap<u64, T>,
    keys: &mut VecDeque<u64>,
    capacity: usize,
) {
    if map.len() > capacity {
        if let Some(oldest_key) = keys.pop_front() {
            map.remove(&oldest_key);
        }
    }
}

impl BinancePrivate {
    /// Converts the stored Binance order updates into Bybit-shaped
    /// `FastExecData` so `QuoteGenerator::check_for_fills` can process both
    /// exchanges uniformly. Binance reports cumulative filled quantities and
    /// string fields, which are parsed here into the typed bybit model.
    pub fn into_fastexec(&self) -> VecDeque<FastExecData> {
        let mut arr = VecDeque::new();
        for v in self.executions.values() {
            arr.push_back(FastExecData {
                category: Category::Linear.as_str().to_string(),
                symbol: v.symbol.clone(),
                order_id: v.order_id.to_string(),
                exec_id: v.trade_id.to_string(),
                exec_price: v.average_price.parse().unwrap_or(0.0),
                exec_qty: v.accumulated_qty_filled_trades.parse().unwrap_or(0.0),
                exec_time: v.trade_order_time,
                side: v.side.to_string(),
                seq: v.trade_id as u64,
                order_link_id: v.new_client_order_id.to_string(),
            });
        }
        arr
    }
}
