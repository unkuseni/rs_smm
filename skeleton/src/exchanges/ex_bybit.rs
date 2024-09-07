use bybit::{
    account::AccountManager,
    api::Bybit,
    config::Config,
    general::General,
    market::MarketData,
    model::{
        Category, FastExecData, InstrumentRequest, LeverageRequest, LinearTickerData,
        OrderBookUpdate, OrderData, PositionData, Subscription, Tickers, WalletData,
        WebsocketEvents, WsTrade,
    },
    position::PositionManager,
    trade::Trader,
    ws::Stream as BybitStream,
};
use std::{borrow::Cow, collections::VecDeque, time::Duration};
use tokio::sync::mpsc;

use crate::util::localorderbook::LocalBook;

use super::exchange::{Exchange, PrivateData, TaggedPrivate};

#[derive(Clone, Debug)]
pub struct BybitMarket {
    pub time: u64,
    pub books: Vec<(String, LocalBook)>,
    pub trades: Vec<(String, VecDeque<WsTrade>)>,
    pub tickers: Vec<(String, VecDeque<LinearTickerData>)>,
}

impl Default for BybitMarket {
    fn default() -> Self {
        Self {
            time: 0,
            books: Vec::new(),
            trades: Vec::new(),
            tickers: Vec::new(),
        }
    }
}

unsafe impl Send for BybitMarket {}
unsafe impl Sync for BybitMarket {}

#[derive(Clone, Debug)]
pub struct BybitPrivate {
    pub time: u64,
    pub wallet: VecDeque<WalletData>,
    pub orders: VecDeque<OrderData>,
    pub positions: VecDeque<PositionData>,
    pub executions: VecDeque<FastExecData>,
}

unsafe impl Send for BybitPrivate {}
unsafe impl Sync for BybitPrivate {}

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

#[derive(Clone, Debug, PartialEq)]
pub struct BybitClient {
    pub key: String,
    pub secret: String,
}

impl Exchange for BybitClient {

    type Quoter<'a> = Trader<'a>;

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
        let account: AccountManager = Bybit::new(
            Some(Cow::Borrowed(&self.key)),
            Some(Cow::Borrowed(&self.secret)),
        );
        let rate;
        let response = account.get_fee_rate(Category::Linear, None).await;
        if let Ok(v) = response {
            rate = v.result.list[0].maker_fee_rate.parse().unwrap();
        } else {
            rate = 0.0000_f64;
        }
        rate
    }

    async fn set_leverage(&self, symbol: &str, leverage: u16) -> Result<String, String> {
        let account: PositionManager = Bybit::new(
            Some(Cow::Borrowed(&self.key)),
            Some(Cow::Borrowed(&self.secret)),
        );
        let req = LeverageRequest {
            category: Category::Linear,
            symbol: Cow::Borrowed(symbol),
            leverage: leverage as i8,
        };
        match account.set_leverage(req).await {
            Ok(res) => Ok(res.ret_msg),
            Err(e) => Err(e.to_string()),
        }
    }

    fn trader<'a>(&'a self) -> Trader<'a> {
        let config = {
            let x = Config::default();
            x.set_recv_window(2500)
        };
        let trader: Trader = Bybit::new_with_config(
            &config,
            Some(Cow::Borrowed(&self.key)),
            Some(Cow::Borrowed(&self.secret)),
        );
        trader
    }
}

impl BybitClient {
    pub async fn market_subscribe(
        &self,
        symbol: Vec<String>,
        sender: mpsc::UnboundedSender<BybitMarket>,
    ) {
        let delay = 50;
        let market: BybitStream = Bybit::new(None, None);
        let category: Category = Category::Linear;
        let request_args = build_requests(&symbol);
        let mut market_data = BybitMarket::default();
        let request = Subscription::new(
            "subscribe",
            request_args.iter().map(String::as_str).collect(),
        );
        market_data.books = symbol
            .iter()
            .map(|s| (s.to_string(), LocalBook::new()))
            .collect::<Vec<(String, LocalBook)>>();
        for (s, b) in &mut market_data.books {
            let cl: MarketData = Bybit::new(None, None);
            let req = InstrumentRequest::new(category, Some(s), None, None, None);
            if let Ok(res) = cl.get_futures_instrument_info(req).await {
                b.tick_size = res.result.list[0].price_filter.tick_size;
                if let Some(v) = &res.result.list[0].lot_size_filter.qty_step {
                    b.lot_size = v.parse::<f64>().unwrap_or(0.0);
                }
                if let Some(v) = &res.result.list[0].lot_size_filter.post_only_max_order_qty {
                    b.post_only_max = v.parse::<f64>().unwrap_or(0.0);
                }
                b.min_order_size = res.result.list[0].lot_size_filter.min_order_qty;
                if let Some(v) = &res.result.list[0].lot_size_filter.min_order_amt {
                    b.min_notional = v.parse::<f64>().unwrap_or(0.0);
                }
            }
        }
        market_data.trades = symbol
            .iter()
            .map(|s| (s.to_string(), VecDeque::with_capacity(5000)))
            .collect::<Vec<(String, VecDeque<WsTrade>)>>();
        market_data.tickers = symbol
            .iter()
            .map(|s| (s.to_string(), VecDeque::with_capacity(10)))
            .collect::<Vec<(String, VecDeque<LinearTickerData>)>>();
        let handler = move |event| {
            match event {
                WebsocketEvents::OrderBookEvent(OrderBookUpdate {
                    topic,
                    data,
                    timestamp,
                    ..
                }) => {
                    let sym = topic.split('.').nth(2).unwrap();
                    let book = &mut market_data
                        .books
                        .iter_mut()
                        .find(|(s, _)| s == sym)
                        .unwrap()
                        .1;

                    if topic == format!("orderbook.1.{}", sym) {
                        book.update_bba(data.bids, data.asks, timestamp);
                        market_data.time = timestamp;
                    } else {
                        book.update(data.bids, data.asks, timestamp);
                    }
                }
                WebsocketEvents::TickerEvent(tick) => {
                    let sym = tick.topic.split('.').nth(1).unwrap();
                    let ticker = &mut market_data
                        .tickers
                        .iter_mut()
                        .find(|(s, _)| s == sym)
                        .unwrap()
                        .1;
                    if ticker.len() == ticker.capacity() || (ticker.capacity() - ticker.len()) <= 1
                    {
                        for _ in 0..2 {
                            ticker.pop_front();
                        }
                    }
                    let d = match tick.data {
                        Tickers::Linear(data) => data,
                        _ => unreachable!(),
                    };
                    ticker.push_back(d);
                }
                WebsocketEvents::TradeEvent(data) => {
                    let sym = data.topic.split('.').nth(1).unwrap();
                    let trades = &mut market_data
                        .trades
                        .iter_mut()
                        .find(|(s, _)| s == sym)
                        .unwrap()
                        .1;
                    if trades.len() == trades.capacity()
                        || (trades.capacity() - trades.len()) <= data.data.len()
                    {
                        for _ in 0..data.data.len() {
                            trades.pop_front();
                        }
                    }
                    trades.extend(data.data);
                }
                _ => {
                    eprintln!("Unhandled event: {:#?}", event);
                }
            }
            let _ = sender.send(market_data.clone());
            Ok(())
        };
        loop {
            match market
                .ws_subscribe(request.clone(), category, handler.clone())
                .await
            {
                Ok(_) => {
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
            }
        }
    }

    pub async fn private_subscribe(
        &self,
        sender: mpsc::UnboundedSender<TaggedPrivate>,
        symbol: String,
    ) {
        let delay = 50;
        let user_stream: BybitStream = BybitStream::new(
            Some(Cow::Borrowed(&self.key)),    // API key
            Some(Cow::Borrowed(&self.secret)), // Secret Key
        );
        let request_args = {
            let mut args = vec![];
            args.push("position.linear".to_string());
            args.push("execution.fast".to_string());
            args.push("order.linear".to_string());
            args.push("wallet".to_string());
            args
        };
        let mut private_data = BybitPrivate::default();
        let request = Subscription::new(
            "subscribe",
            request_args.iter().map(String::as_str).collect(),
        );
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
                _ => {
                    eprintln!("Unhandled event: {:#?}", event);
                }
            }
            let tagged_data =
                TaggedPrivate::new(symbol.clone(), PrivateData::Bybit(private_data.clone()));
            sender.send(tagged_data).unwrap();
            Ok(())
        };
        loop {
            match user_stream
                .ws_priv_subscribe(request.clone(), handler.clone())
                .await
            {
                Ok(_) => {
                    println!("Subscription successful");
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
                Err(e) => {
                    eprintln!("Subscription error: {}", e);
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
            }
        }
    }
}

/// Builds the request arguments for the WebSocket connection.
///
/// # Arguments
///
/// * `symbol` - The symbols to request data for.
///
/// # Returns
///
/// A vector of strings, each representing a different request.
fn build_requests(symbol: &[String]) -> Vec<String> {
    let mut request_args = vec![];

    // Building book requests
    let book_req: Vec<String> = symbol
        .iter()
        .flat_map(|sym| vec![(1, sym), (50, sym), (500, sym)])
        .map(|(num, sym)| format!("orderbook.{}.{}", num, sym.to_uppercase()))
        .collect();
    request_args.extend(book_req);

    // Building tickers requests
    let tickers_req: Vec<String> = symbol
        .iter()
        .map(|sub| format!("tickers.{}", sub.to_uppercase()))
        .collect();
    request_args.extend(tickers_req);

    // Building trade requests
    let trade_req: Vec<String> = symbol
        .iter()
        .map(|sub| format!("publicTrade.{}", sub.to_uppercase()))
        .collect();
    request_args.extend(trade_req);

    request_args
}
