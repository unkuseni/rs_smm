use std::collections::{HashMap, VecDeque};
use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::Duration;

use binance::config::Config;
use binance::futures::account::FuturesAccount;
use binance::futures::general::FuturesGeneral;
use binance::futures::model::Filters::PriceFilter;
use binance::futures::model::{AccountInformation, OrderTradeEvent, OrderUpdate};
use binance::futures::userstream::FuturesUserStream;
use binance::model::{
    AccountUpdateEvent, Asks, Bids, BookTickerEvent, ContinuousKline, DepthOrderBookEvent,
    EventBalance, EventPosition, LiquidationOrder,
};
use binance::{api::Binance, futures::websockets::*, general::General};
use bybit::model::{Category, FastExecData, WsTrade};
use tokio::sync::mpsc;

use crate::util::localorderbook::{LocalBook, ProcessAsks, ProcessBids};

use super::exchange::{PrivateData, ProcessTrade, TaggedPrivate};
#[derive(Clone, Debug)]
pub struct BinanceMarket {
    pub time: u64,
    pub books: Vec<(String, LocalBook)>,
    pub klines: Vec<(String, VecDeque<ContinuousKline>)>,
    pub trades: Vec<(String, VecDeque<WsTrade>)>,
    pub tickers: Vec<(String, VecDeque<BookTickerEvent>)>,
    pub liquidations: Vec<(String, VecDeque<LiquidationOrder>)>,
}

unsafe impl Send for BinanceMarket {}
unsafe impl Sync for BinanceMarket {}

impl Default for BinanceMarket {
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

#[derive(Clone, Debug)]
pub struct BinancePrivate {
    pub time: u64,
    pub wallet: VecDeque<EventBalance>,
    pub orders: HashMap<u64, OrderUpdate>,
    pub positions: VecDeque<EventPosition>,
    pub executions: HashMap<u64, OrderUpdate>,
}

unsafe impl Send for BinancePrivate {}
unsafe impl Sync for BinancePrivate {}

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

#[derive(Clone, Debug, PartialEq)]
pub struct BinanceClient {
    pub key: String,
    pub secret: String,
}

impl Default for BinanceClient {
    fn default() -> Self {
        Self {
            key: String::new(),
            secret: String::new(),
        }
    }
}

impl BinanceClient {
    pub fn init(key: String, secret: String) -> Self {
        Self { key, secret }
    }
    pub fn exchange_time(&self) -> u64 {
        let general: General = Binance::new(None, None);
        match general.get_server_time() {
            Ok(v) => v.server_time,
            Err(_) => 0,
        }
    }

    pub fn market_subscribe(
        &self,
        symbol: Vec<String>,
        sender: mpsc::UnboundedSender<BinanceMarket>,
    ) {
        let mut delay = 600;
        let keep_running = AtomicBool::new(true);
        let request = bin_build_requests(&symbol);

        let mut market_data = BinanceMarket::default();
        market_data.books = symbol
            .iter()
            .map(|s| (s.to_string(), LocalBook::new()))
            .collect::<Vec<(String, LocalBook)>>();
        for (s, b) in &mut market_data.books {
            let cl_symbol = format!("{}", s);
            let cl: FuturesGeneral = Binance::new(None, None);
            match cl.get_symbol_info(cl_symbol) {
                Ok(v) => {
                    let price_filter = match &v.filters[0] {
                        PriceFilter { tick_size, .. } => tick_size.parse().unwrap_or(0.0),
                        _ => 0.0,
                    };
                    b.tick_size = price_filter;
                    b.min_order_size = {
                        match &v.filters[1] {
                            binance::model::Filters::LotSize { min_qty, .. } => {
                                min_qty.parse().unwrap_or(0.0)
                            }
                            _ => 0.0,
                        }
                    };
                    b.min_notional = match &v.filters[5] {
                        binance::model::Filters::MinNotional { notional, .. } => {
                            notional.clone().unwrap().parse().unwrap_or(0.0)
                        }
                        _ => 0.0,
                    };
                }
                Err(_) => {
                    b.tick_size = 0.0;
                }
            }
        }
        market_data.klines = symbol
            .iter()
            .map(|s| (s.to_string(), VecDeque::with_capacity(2000)))
            .collect::<Vec<(String, VecDeque<ContinuousKline>)>>();
        market_data.liquidations = symbol
            .iter()
            .map(|s| (s.to_string(), VecDeque::with_capacity(2000)))
            .collect::<Vec<(String, VecDeque<LiquidationOrder>)>>();
        market_data.trades = symbol
            .iter()
            .map(|s| (s.to_string(), VecDeque::with_capacity(5000)))
            .collect::<Vec<(String, VecDeque<WsTrade>)>>();
        market_data.tickers = symbol
            .iter()
            .map(|s| (s.to_string(), VecDeque::with_capacity(10)))
            .collect::<Vec<(String, VecDeque<BookTickerEvent>)>>();

        let handler = move |event| {
            match event {
                FuturesWebsocketEvent::DepthOrderBook(DepthOrderBookEvent {
                    symbol,
                    event_time,
                    bids,
                    asks,
                    ..
                }) => {
                    let sym = symbol.as_str();
                    let book = &mut market_data
                        .books
                        .iter_mut()
                        .find(|(s, _)| s == sym)
                        .unwrap()
                        .1;
                    let new_bids = {
                        let mut arr = Vec::new();
                        for bid in bids {
                            arr.push(Bids::process_bids(bid));
                        }
                        arr
                    };
                    let new_asks = {
                        let mut arr = Vec::new();
                        for ask in asks {
                            arr.push(Asks::process_asks(ask));
                        }
                        arr
                    };
                    if new_bids.len() == new_asks.len()
                        && (new_bids.len() == 5 || new_bids.len() == 10 || new_bids.len() == 20)
                    {
                        // Process when the lengths are equal and equal to 5, 10, or 20
                        book.update_binance_bba(new_bids.clone(), new_asks.clone(), event_time);
                    } else {
                        // Process when the lengths are not equal or not equal to 5, 10, or 20
                        book.update(new_bids.clone(), new_asks.clone(), event_time);
                    }

                    market_data.time = event_time;
                }
                FuturesWebsocketEvent::AggrTrades(agg) => {
                    let sym = agg.symbol.as_str();
                    let trades = &mut market_data
                        .trades
                        .iter_mut()
                        .find(|(s, _)| s == sym)
                        .unwrap()
                        .1;
                    if trades.len() == trades.capacity() || (trades.capacity() - trades.len()) <= 5
                    {
                        for _ in 0..10 {
                            trades.pop_front();
                        }
                    }
                    trades.push_back(agg.process_trade());
                }
                FuturesWebsocketEvent::Liquidation(liquidation) => {
                    let sym = liquidation.liquidation_order.symbol.as_str();
                    let liquidations = &mut market_data
                        .liquidations
                        .iter_mut()
                        .find(|(s, _)| s == sym)
                        .unwrap()
                        .1;
                    if liquidations.len() == liquidations.capacity()
                        || (liquidations.capacity() - liquidations.len()) <= 5
                    {
                        for _ in 0..10 {
                            liquidations.pop_front();
                        }
                    }
                    liquidations.push_back(liquidation.liquidation_order);
                }
                FuturesWebsocketEvent::ContinuousKline(kline) => {
                    let sym = kline.pair.as_str();
                    let kline_data = &mut market_data
                        .klines
                        .iter_mut()
                        .find(|(s, _)| s == sym)
                        .unwrap()
                        .1;
                    if kline_data.len() == kline_data.capacity()
                        || (kline_data.capacity() - kline_data.len()) <= 10
                    {
                        for _ in 0..30 {
                            kline_data.pop_front();
                        }
                    }
                    kline_data.push_back(kline.kline);
                }

                FuturesWebsocketEvent::BookTicker(ticker) => {
                    let sym = ticker.symbol.as_str();
                    let ticker_data = &mut market_data
                        .tickers
                        .iter_mut()
                        .find(|(s, _)| s == sym)
                        .unwrap()
                        .1;
                    if ticker_data.len() == ticker_data.capacity()
                        || (ticker_data.capacity() - ticker_data.len()) <= 10
                    {
                        for _ in 0..10 {
                            ticker_data.pop_front();
                        }
                    }
                    ticker_data.push_back(ticker);
                }
                _ => {}
            }
            let _ = sender.send(market_data.clone());
            Ok(())
        };

        let mut market: FuturesWebSockets<'_> = FuturesWebSockets::new(handler);
        loop {
            market
                .connect_multiple_streams(&FuturesMarket::USDM, &request)
                .unwrap();
            // check error
            if let Err(e) = market.event_loop(&keep_running) {
                eprintln!("Error: {}", e);
                thread::sleep(Duration::from_millis(delay));
                delay *= 2;
            }
        }
    }
    pub fn binance_trader(&self) -> FuturesAccount {
        let config = {
            let x = Config::default();
            x.set_recv_window(2500)
        };
        let trader: FuturesAccount =
            Binance::new_with_config(Some(self.key.clone()), Some(self.secret.clone()), &config);
        trader
    }

    pub fn private_subscribe(&self, sender: mpsc::UnboundedSender<TaggedPrivate>, symbol: String) {
        let mut delay = 600;
        let keep_running = AtomicBool::new(true); // Used to control the event loop
        let user_stream: FuturesUserStream = Binance::new(Some(self.key.clone()), None);

        let mut private_data = BinancePrivate::default();
        let mut orders_keys: VecDeque<u64> = VecDeque::new();
        let mut executions_keys: VecDeque<u64> = VecDeque::new();
        let handler = |event: FuturesWebsocketEvent| {
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
                        for _ in 0..(data.positions.len() - private_data.positions.len()) {
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
        if let Ok(answer) = user_stream.start() {
            println!("Data Stream Started ...");
            let listen_key = answer.listen_key;
            let mut web_socket: FuturesWebSockets<'_> = FuturesWebSockets::new(handler);
            loop {
                web_socket
                    .connect(&FuturesMarket::USDM, &listen_key)
                    .unwrap(); // check error
                if let Err(e) = web_socket.event_loop(&keep_running) {
                    println!("Error: {}", e);
                    thread::sleep(Duration::from_millis(delay));
                    delay *= 2
                }
            }
        } else {
            println!("Not able to start an User Stream (Check your API_KEY)");
        }
    }

    pub fn fee_rate(&self) -> AccountInformation {
        let client: FuturesAccount =
            Binance::new(Some(self.key.clone()), Some(self.secret.clone()));
        let info = client.account_information().unwrap();
        info
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
    let kline_req: Vec<String> = symbol
        .iter()
        .map(|sub| sub.to_lowercase())
        .flat_map(|sym| vec![("5m", sym.clone()), ("1m", sym.clone())])
        .map(|(interval, sub)| format!("{}_perpetual@@continuousKline_{}", sub, interval))
        .collect();
    request_args.extend(kline_req);
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
    let tickers: Vec<String> = symbol
        .iter()
        .map(|sub| sub.to_lowercase())
        .map(|sub| format!("{}@bookTicker", sub))
        .collect();
    request_args.extend(tickers);
    let liq_req: Vec<String> = symbol
        .iter()
        .map(|sub| sub.to_lowercase())
        .map(|sub| format!("{}@forceOrder", sub))
        .collect();
    request_args.extend(liq_req);
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
    pub fn into_fastexec(&self) -> VecDeque<FastExecData> {
        let mut arr = VecDeque::new();
        for (_, v) in self.executions.iter() {
            arr.push_back(FastExecData {
                category: Category::Linear.as_str().to_string(),
                symbol: v.symbol.clone(),
                order_id: v.order_id.to_string(),
                exec_id: v.trade_id.to_string(),
                exec_price: v.average_price.to_string(),
                exec_qty: v.accumulated_qty_filled_trades.to_string(),
                exec_time: v.trade_order_time.to_string(),
                side: v.side.to_string(),
                seq: v.trade_id as u64,
                order_link_id: v.new_client_order_id.to_string(),
            });
        }
        arr
    }
}
