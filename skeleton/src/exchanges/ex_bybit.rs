use bybit::{
    AccountManager, Bybit, Category, Config, FastExecData, General, LeverageRequest,
    OrderBookUpdate, OrderData, PositionData, PositionManager, Stream, Subscription, Trader,
    WalletData, WebsocketEvents, WsTrade,
};
use std::{borrow::Cow, collections::VecDeque, fmt, sync::Arc, time::Duration};
use tokio::sync::mpsc;

use crate::util::localorderbook::LocalBook;

use super::exchange::{Exchange, MarketEvent, PrivateData, TaggedPrivate};

/// A market snapshot for one symbol, referencing the loader's authoritative
/// book and trade buffers through `Arc` so sending a message is a cheap
/// reference-count bump rather than a full clone.
#[derive(Clone, Debug, Default)]
pub struct BybitMarket {
    pub time: u64,
    pub books: Vec<(String, Arc<LocalBook>)>,
    pub trades: Vec<(String, Arc<VecDeque<WsTrade>>)>,
}

#[derive(Clone, Debug)]
pub struct BybitPrivate {
    pub time: u64,
    pub wallet: VecDeque<WalletData>,
    pub orders: VecDeque<OrderData>,
    pub positions: VecDeque<PositionData>,
    pub executions: VecDeque<FastExecData>,
}

impl Default for BybitPrivate {
    fn default() -> Self {
        Self {
            time: 0,
            wallet: VecDeque::with_capacity(20),
            orders: VecDeque::with_capacity(1500),
            positions: VecDeque::with_capacity(500),
            executions: VecDeque::with_capacity(2000),
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct BybitClient {
    pub key: String,
    pub secret: String,
}

// Redact credentials so `{:?}` of any struct holding a client never leaks keys.
impl fmt::Debug for BybitClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BybitClient")
            .field("key", &"<redacted>")
            .field("secret", &"<redacted>")
            .finish()
    }
}

impl Exchange for BybitClient {
    type Quoter = Trader;

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
        let general: General = Bybit::new(None, None);
        general
            .get_server_time()
            .await
            .map(|data| data.result.time_nano / 1_000_000)
            .unwrap_or(0)
    }

    async fn fees(&self) -> f64 {
        let account: AccountManager = Bybit::new(Some(self.key.clone()), Some(self.secret.clone()));
        let response = account.get_fee_rate(Category::Linear, None).await;
        response
            .ok()
            .and_then(|v| {
                v.result
                    .list
                    .first()
                    .map(|rate| rate.maker_fee_rate.parse().ok())
            })
            .flatten()
            .unwrap_or(0.0)
    }

    async fn set_leverage(&self, symbol: &str, leverage: u16) -> Result<String, String> {
        let account: PositionManager =
            Bybit::new(Some(self.key.clone()), Some(self.secret.clone()));
        let leverage = leverage.clamp(1, 100).to_string();
        let req = LeverageRequest {
            category: Category::Linear,
            symbol: Cow::Borrowed(symbol),
            buy_leverage: leverage.clone(),
            sell_leverage: leverage,
        };
        match account.set_leverage(req).await {
            Ok(res) => Ok(res.ret_msg),
            Err(e) => Err(e.to_string()),
        }
    }

    fn trader(&self) -> Trader {
        let config = Config::default().set_recv_window(5000);
        Bybit::new_with_config(&config, Some(self.key.clone()), Some(self.secret.clone()))
    }
}

impl BybitClient {
    /// Subscribes to public market streams for the given symbols and forwards
    /// lightweight deltas (`MarketEvent`) to the sender.
    pub async fn market_subscribe(
        &self,
        symbol: Vec<String>,
        sender: mpsc::UnboundedSender<MarketEvent>,
    ) {
        let market: Stream = Bybit::new(None, None);
        let category: Category = Category::Linear;
        let request_args = build_requests(&symbol);
        let request = Subscription::new(
            "subscribe",
            request_args.iter().map(String::as_str).collect(),
        );
        // The handler signature is fixed by the bybit crate and returns a
        // large error type; boxing it here is not possible.
        #[allow(clippy::result_large_err)]
        let handler = move |event| {
            match event {
                WebsocketEvents::OrderBookEvent(OrderBookUpdate {
                    topic,
                    data,
                    timestamp,
                    ..
                }) => {
                    let sym = topic.split('.').nth(2).unwrap_or_default().to_string();
                    // `orderbook.1.` is the best-bid/ask snapshot topic.
                    let bba = topic.starts_with("orderbook.1.");
                    let _ = sender.send(MarketEvent::Book {
                        symbol: sym,
                        bids: data.bids,
                        asks: data.asks,
                        timestamp,
                        bba,
                    });
                }
                WebsocketEvents::TradeEvent(data) => {
                    let sym = data.topic.split('.').nth(1).unwrap_or_default().to_string();
                    for trade in data.data {
                        let _ = sender.send(MarketEvent::Trade {
                            symbol: sym.clone(),
                            trade,
                        });
                    }
                }
                _ => {}
            }
            Ok(())
        };
        // Reconnect loop with capped exponential backoff.
        let mut delay = 50_u64;
        loop {
            match market
                .ws_subscribe(request.clone(), category, handler.clone())
                .await
            {
                Ok(_) => {
                    delay = 50;
                }
                Err(e) => {
                    eprintln!("Market subscription error: {}", e);
                    delay = (delay * 2).min(5000);
                }
            }
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
    }

    pub async fn private_subscribe(
        &self,
        sender: mpsc::UnboundedSender<TaggedPrivate>,
        symbol: String,
    ) {
        let user_stream: Stream = Bybit::new(
            Some(self.key.clone()),    // API key
            Some(self.secret.clone()), // Secret Key
        );
        let request_args = [
            "position.linear",
            "execution.fast",
            "order.linear",
            "wallet",
        ];
        let mut private_data = BybitPrivate::default();
        let request = Subscription::new("subscribe", request_args.to_vec());
        // The handler signature is fixed by the bybit crate and returns a
        // large error type; boxing it here is not possible.
        #[allow(clippy::result_large_err)]
        let handler = move |event| {
            match event {
                WebsocketEvents::Wallet(data) => {
                    private_data.time = data.creation_time;
                    if private_data.wallet.len() == private_data.wallet.capacity()
                        || (private_data.wallet.capacity() - private_data.wallet.len())
                            <= data.data.len()
                    {
                        for _ in 0..data.data.len() {
                            private_data.wallet.pop_front();
                        }
                    }
                    private_data.wallet.extend(data.data);
                }
                WebsocketEvents::PositionEvent(data) => {
                    private_data.time = data.creation_time;
                    if private_data.positions.len() == private_data.positions.capacity()
                        || (private_data.positions.capacity() - private_data.positions.len())
                            <= data.data.len()
                    {
                        for _ in 0..data.data.len() {
                            private_data.positions.pop_front();
                        }
                    }
                    private_data.positions.extend(data.data);
                }
                WebsocketEvents::FastExecEvent(data) => {
                    private_data.time = data.creation_time;
                    if private_data.executions.len() == private_data.executions.capacity()
                        || (private_data.executions.capacity() - private_data.executions.len())
                            <= data.data.len()
                    {
                        for _ in 0..data.data.len() {
                            private_data.executions.pop_front();
                        }
                    }
                    private_data.executions.extend(data.data);
                }
                WebsocketEvents::OrderEvent(data) => {
                    private_data.time = data.creation_time;
                    if private_data.orders.len() == private_data.orders.capacity()
                        || (private_data.orders.capacity() - private_data.orders.len())
                            <= data.data.len()
                    {
                        for _ in 0..data.data.len() {
                            private_data.orders.pop_front();
                        }
                    }
                    private_data.orders.extend(data.data);
                }
                _ => {}
            }
            let tagged_data =
                TaggedPrivate::new(symbol.clone(), PrivateData::Bybit(private_data.clone()));
            let _ = sender.send(tagged_data);
            Ok(())
        };
        let mut delay = 50_u64;
        loop {
            match user_stream
                .ws_priv_subscribe(request.clone(), handler.clone())
                .await
            {
                Ok(_) => {
                    delay = 50;
                }
                Err(e) => {
                    eprintln!("Subscription error: {}", e);
                    delay = (delay * 2).min(5000);
                }
            }
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
    }
}

/// Builds the request arguments for the WebSocket connection.
fn build_requests(symbol: &[String]) -> Vec<String> {
    let mut request_args = vec![];

    // Book requests: best-bid/ask plus 50- and 500-level depth snapshots.
    let book_req: Vec<String> = symbol
        .iter()
        .flat_map(|sym| vec![(1, sym), (50, sym), (500, sym)])
        .map(|(num, sym)| format!("orderbook.{}.{}", num, sym.to_uppercase()))
        .collect();
    request_args.extend(book_req);

    // Trade requests.
    let trade_req: Vec<String> = symbol
        .iter()
        .map(|sub| format!("publicTrade.{}", sub.to_uppercase()))
        .collect();
    request_args.extend(trade_req);

    request_args
}
