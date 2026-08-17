use std::fmt::Debug;

use binance::model::AggrTradesEvent;
use bybit::{Ask, Bid, TickDirection, WsTrade};

use super::{
    ex_binance::{BinanceClient, BinanceMarket, BinancePrivate},
    ex_bybit::{BybitClient, BybitMarket, BybitPrivate},
};

use std::future::Future;

pub trait Exchange {
    type Quoter;
    fn default() -> Self;
    fn init<K: Into<String>>(key: K, secret: K) -> Self;
    fn time(&self) -> impl Future<Output = u64>;
    fn fees(&self) -> impl Future<Output = f64>;
    fn set_leverage(
        &self,
        symbol: &str,
        leverage: u16,
    ) -> impl Future<Output = Result<String, String>>;
    fn trader(&self) -> Self::Quoter;
}

#[derive(Clone, Debug)]
pub enum Client {
    Bybit(BybitClient),
    Binance(BinanceClient),
}

#[derive(Clone, Debug)]
pub enum PrivateData {
    Bybit(BybitPrivate),
    Binance(BinancePrivate),
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

/// A lightweight market-data delta emitted by the websocket handlers.
///
/// The loaders in `ss` apply these deltas to the authoritative books/trades
/// they own, which keeps the per-event channel payload small instead of
/// cloning the entire market snapshot on every websocket event.
#[derive(Clone, Debug)]
pub enum MarketEvent {
    Book {
        symbol: String,
        bids: Vec<Bid>,
        asks: Vec<Ask>,
        timestamp: u64,
        /// Whether this is a best-bid/ask snapshot rather than a depth update.
        bba: bool,
    },
    Trade {
        symbol: String,
        trade: WsTrade,
    },
}

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
            // Aggregated trades carry no tick-direction info.
            tick_direction: TickDirection::ZeroPlusTick,
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
