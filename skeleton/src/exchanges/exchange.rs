use std::fmt::Debug;

use binance::{futures::account::FuturesAccount, model::AggrTradesEvent};
use bybit::{model::WsTrade, trade::Trader};

use super::{
    ex_binance::{BinanceClient, BinanceMarket, BinancePrivate},
    ex_bybit::{BybitClient, BybitMarket, BybitPrivate},
};

use std::future::Future;

pub trait Exchange {
    fn default() -> Self;
    fn init<K: Into<String>>(key: K, secret: K) -> Self;
    fn time(&self) -> impl Future<Output = u64>;
    fn fees(&self) -> impl Future<Output = f64>;
    fn set_leverage(&self, symbol: &str, leverage: u16) -> impl Future<Output = Result<String, String>>;
    fn trader<'a>(&'a self) -> Quoter<'a>;
}

#[derive(Clone, Debug)]
pub enum Client {
    Bybit(BybitClient),
    Binance(BinanceClient),
}


pub enum Quoter<'a> {
    Bybit(Trader<'a>),
    Binance(FuturesAccount),
}



#[derive(Clone, Debug)]
pub enum PrivateData {
    Bybit(BybitPrivate),
    Binance(BinancePrivate),
}

impl PrivateData {
    pub fn unwrap(self) -> Box<dyn Debug> {
        match self {
            Self::Bybit(v) => Box::new(v),
            Self::Binance(v) => Box::new(v),
        }
    }
}

#[derive(Clone, Debug)]
pub struct TaggedPrivate {
    pub symbol: String,
    pub data: PrivateData,
}

impl TaggedPrivate {
    pub fn new(symbol: String, data: PrivateData) -> Self {
        TaggedPrivate { symbol, data }
    }
}

unsafe impl Send for TaggedPrivate {}
unsafe impl Sync for TaggedPrivate {}

#[derive(Debug)]
pub enum MarketMessage {
    Bybit(BybitMarket),
    Binance(BinanceMarket),
}

impl Clone for MarketMessage {
    fn clone(&self) -> Self {
        match self {
            Self::Bybit(v) => Self::Bybit(v.clone()),
            Self::Binance(v) => Self::Binance(v.clone()),
        }
    }
}

impl MarketMessage {
    pub fn unwrap(self) -> Box<dyn Debug> {
        match self {
            MarketMessage::Bybit(v) => Box::new(v),
            MarketMessage::Binance(v) => Box::new(v),
        }
    }
}

pub trait ProcessTrade {
    fn process_trade(&self) -> WsTrade;
}

impl ProcessTrade for AggrTradesEvent {
    fn process_trade(&self) -> WsTrade {
        WsTrade {
            timestamp: self.event_time,
            symbol: self.symbol.clone(),
            price: self.price.parse::<f64>().unwrap(),
            volume: self.qty.parse::<f64>().unwrap(),
            side: self.event_type.clone(),
            tick_direction: "Zero".to_string(),
            id: self.aggregated_trade_id.to_string(),
            buyer_is_maker: self.is_buyer_maker,
        }
    }
}

impl ProcessTrade for WsTrade {
    fn process_trade(&self) -> WsTrade {
        self.clone()
    }
}
