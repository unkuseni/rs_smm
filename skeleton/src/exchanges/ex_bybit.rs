use std::time::Duration;

use bybit::{
    api::Bybit,
    general::General,
    model::{
        Category, KlineData, LinearTickerData, LiquidationData, OrderBookUpdate, Subscription,
        Tickers, WebsocketEvents, WsKline, WsTrade,
    },
    ws::Stream as BybitStream,
};
use tokio::sync::mpsc;

use crate::util::localorderbook::LocalBook;

use super::exchange::Exchange;

#[derive(Clone, Debug)]
pub struct BybitMarket {
    pub time: u64,
    pub books: Vec<(String, LocalBook)>,
    pub klines: Vec<(String, Vec<KlineData>)>,
    pub trades: Vec<(String, Vec<WsTrade>)>,
    pub tickers: Vec<(String, Vec<LinearTickerData>)>,
    pub liquidations: Vec<(String, Vec<LiquidationData>)>,
}

unsafe impl Send for BybitMarket {}
unsafe impl Sync for BybitMarket {}

#[derive(Clone, Debug)]
pub struct BybitPrivate {
    pub time: u64,
    pub wallet: String,
    pub orders: String,
    pub positions: String,
    pub executions: String,
}
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

impl Exchange for BybitClient {
    fn init(key: String, secret: String) -> Self {
        Self { key, secret }
    }

    async fn exchange_time(&self) -> u64 {
        let general: General = Bybit::new(None, None);
        let response = general.get_server_time().await;
        if let Ok(v) = response {
            println!("Server time: {}", v.time_second);
            v.time_second
        } else {
            0
        }
    }

    async fn market_subscribe(
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
        market_data.klines = symbol
            .iter()
            .map(|s| (s.to_string(), Vec::new()))
            .collect::<Vec<(String, Vec<KlineData>)>>();

        market_data.liquidations = symbol
            .iter()
            .map(|s| (s.to_string(), Vec::new()))
            .collect::<Vec<(String, Vec<LiquidationData>)>>();
        market_data.trades = symbol
            .iter()
            .map(|s| (s.to_string(), Vec::new()))
            .collect::<Vec<(String, Vec<WsTrade>)>>();
        market_data.tickers = symbol
            .iter()
            .map(|s| (s.to_string(), Vec::new()))
            .collect::<Vec<(String, Vec<LinearTickerData>)>>();
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
                        sender.send(market_data.clone()).unwrap();
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
                    *ticker = vec![match tick.data {
                        Tickers::Linear(data) => data,
                        _ => unreachable!(),
                    }];
                }
                WebsocketEvents::TradeEvent(data) => {
                    let sym = data.topic.split('.').nth(1).unwrap();
                    let trades = &mut market_data
                        .trades
                        .iter_mut()
                        .find(|(s, _)| s == sym)
                        .unwrap()
                        .1;
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

                    liquidations.push(data.data);
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
    async fn private_subscribe(&self, sender: mpsc::UnboundedSender<BybitPrivate>) {
        unimplemented!()
    }
}

fn build_requests(symbol: &[&str]) -> Vec<String> {
    let mut request_args = vec![];

    // Building book requests
    let book_req: Vec<String> = symbol
        .iter()
        .flat_map(|&sym| vec![(1, sym), (50, sym)])
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
