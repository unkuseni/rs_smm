use bybit::{
    account::AccountManager,
    api::Bybit,
    config::Config,
    general::General,
    market::MarketData,
    model::{
        Category, FastExecData, InstrumentRequest, KlineData, LinearTickerData, LiquidationData,
        OrderBookUpdate, OrderData, PositionData, Subscription, Tickers, WalletData,
        WebsocketEvents, WsTrade,
    },
    trade::Trader,
    ws::Stream as BybitStream,
};
use std::{collections::VecDeque, time::Duration};
use tokio::sync::mpsc;

use crate::util::localorderbook::LocalBook;

use super::exchange::PrivateData;

#[derive(Clone, Debug)]
pub struct BybitMarket {
    pub time: u64,
    pub books: Vec<(String, LocalBook)>,
    pub klines: Vec<(String, VecDeque<KlineData>)>,
    pub trades: Vec<(String, VecDeque<WsTrade>)>,
    pub tickers: Vec<(String, VecDeque<LinearTickerData>)>,
    pub liquidations: Vec<(String, VecDeque<LiquidationData>)>,
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

impl Default for BybitMarket {
    fn default() -> Self {
        Self {
            time: 0,
            books: Vec::new(),
            klines: Vec::new(),
            trades: Vec::new(),
            tickers: Vec::new(),
            liquidations: Vec::new(),
        }
    }
}

impl Default for BybitClient {
    fn default() -> Self {
        Self {
            key: String::new(),
            secret: String::new(),
        }
    }
}

impl BybitClient {
    pub fn init(key: String, secret: String) -> Self {
        Self { key, secret }
    }

    pub async fn exchange_time(&self) -> u64 {
        let general: General = Bybit::new(None, None);
        general
            .get_server_time()
            .await
            .map(|data| data.result.time_nano / 1_000_000)
            .unwrap_or(0)
    }

    pub async fn fee_rate(&self, symbol: &str) -> f64 {
        let account: AccountManager = Bybit::new(Some(self.key.clone()), Some(self.secret.clone()));
        let rate;
        let response = account
            .get_fee_rate(Category::Linear, Some(symbol.to_string()))
            .await;
        if let Ok(v) = response {
            rate = v.result.list[0].maker_fee_rate.parse().unwrap();
        } else {
            rate = 0.0000_f64;
        }
        rate
    }
    pub fn bybit_trader(&self) -> Trader {
        let config = {
            let x = Config::default();
            x.set_recv_window(2500)
        };
        let trader: Trader =
            Bybit::new_with_config(&config, Some(self.key.clone()), Some(self.secret.clone()));
        trader
    }

    pub async fn market_subscribe(
        &self,
        symbol: Vec<&str>,
        sender: mpsc::UnboundedSender<BybitMarket>,
    ) {
        let mut delay = 600;
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
            }
        }
        market_data.klines = symbol
            .iter()
            .map(|s| (s.to_string(), VecDeque::with_capacity(2000)))
            .collect::<Vec<(String, VecDeque<KlineData>)>>();

        market_data.liquidations = symbol
            .iter()
            .map(|s| (s.to_string(), VecDeque::with_capacity(2000)))
            .collect::<Vec<(String, VecDeque<LiquidationData>)>>();
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
                        let _ = sender.send(market_data.clone());
                    } else {
                        book.update(data.bids, data.asks, timestamp);
                    }
                }
                WebsocketEvents::KlineEvent(klines) => {
                    let sym = klines.topic.split('.').nth(2).unwrap();
                    let kline = &mut market_data
                        .klines
                        .iter_mut()
                        .find(|(s, _)| s == sym)
                        .unwrap()
                        .1;
                    if kline.len() == kline.capacity()
                        || (kline.capacity() - kline.len()) <= klines.data.len()
                    {
                        for _ in 0..klines.data.len() {
                            kline.pop_front();
                        }
                    }
                    kline.extend(klines.data);
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
                WebsocketEvents::LiquidationEvent(data) => {
                    let sym = data.topic.split('.').nth(1).unwrap();
                    let liquidations = &mut market_data
                        .liquidations
                        .iter_mut()
                        .find(|(s, _)| s == sym)
                        .unwrap()
                        .1;
                    if liquidations.len() == liquidations.capacity()
                        || (liquidations.capacity() - liquidations.len()) <= 5
                    {
                        for _ in 0..5 {
                            liquidations.pop_front();
                        }
                    }
                    liquidations.push_back(data.data);
                }
                _ => {
                    eprintln!("Unhandled event: {:#?}", event);
                }
            }
            Ok(())
        };
        loop {
            match market
                .ws_subscribe(request.clone(), category, handler.clone())
                .await
            {
                Ok(_) => {
                    println!("Subscription successful");
                    break;
                }
                Err(e) => {
                    eprintln!("Subscription error: {}", e);
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    delay *= 2;
                }
            }
        }
    }
    pub async fn private_subscribe(&self, sender: mpsc::UnboundedSender<PrivateData>) {
        let mut delay = 600;
        let user_stream: BybitStream = BybitStream::new(
            Some(self.key.clone()),    // API key
            Some(self.secret.clone()), // Secret Key
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
            sender
                .send(PrivateData::Bybit(private_data.clone()))
                .unwrap();
            Ok(())
        };
        loop {
            match user_stream
                .ws_priv_subscribe(request.clone(), handler.clone())
                .await
            {
                Ok(_) => {
                    println!("Subscription successful");
                    break;
                }
                Err(e) => {
                    eprintln!("Subscription error: {}", e);
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    delay *= 2;
                }
            }
        }
    }
}

fn build_requests(symbol: &[&str]) -> Vec<String> {
    let mut request_args = vec![];

    // Building book requests
    let book_req: Vec<String> = symbol
        .iter()
        .flat_map(|&sym| vec![(1, sym), (50, sym), (500, sym)])
        .map(|(num, sym)| format!("orderbook.{}.{}", num, sym.to_uppercase()))
        .collect();
    request_args.extend(book_req);

    // Building kline requests
    let kline_req: Vec<String> = symbol
        .iter()
        .flat_map(|&sym| vec![("5", sym), ("1", sym)])
        .map(|(interval, sym)| format!("kline.{}.{}", interval, sym.to_uppercase()))
        .collect();
    request_args.extend(kline_req);

    // Building tickers requests
    let tickers_req: Vec<String> = symbol
        .iter()
        .map(|&sub| format!("tickers.{}", sub.to_uppercase()))
        .collect();
    request_args.extend(tickers_req);

    // Building trade requests
    let trade_req: Vec<String> = symbol
        .iter()
        .map(|&sub| format!("publicTrade.{}", sub.to_uppercase()))
        .collect();
    request_args.extend(trade_req);

    // Building liquidation requests
    let liq_req: Vec<String> = symbol
        .iter()
        .map(|&sub| format!("liquidation.{}", sub.to_uppercase()))
        .collect();
    request_args.extend(liq_req);

    request_args
}
