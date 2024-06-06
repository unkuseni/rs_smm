use std::fmt::Debug;

use binance::model::AggrTradesEvent;
use bybit::model::WsTrade;

use super::{
    ex_binance::{BinanceClient, BinanceMarket, BinancePrivate},
    ex_bybit::{BybitClient, BybitMarket, BybitPrivate},
};

#[derive(Clone, Debug, PartialEq)]
pub enum ExchangeClient {
    Bybit(BybitClient),
    Binance(BinanceClient),
}

impl ExchangeClient {
    pub fn unwrap(self) -> Box<dyn Debug> {
        match self {
            Self::Bybit(v) => Box::new(v),
            Self::Binance(v) => Box::new(v),
        }
    }
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
