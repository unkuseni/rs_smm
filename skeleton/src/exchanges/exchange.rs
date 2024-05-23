use binance::model::AggrTradesEvent;
use bybit::model::WsTrade;

use super::{ex_binance::{BinanceClient, BinanceMarket}, ex_bybit::{BybitClient, BybitMarket}};

#[derive(Clone, Debug, PartialEq)]
pub enum ExchangeClient {
    Bybit(BybitClient),
    Binance(BinanceClient),
}

#[derive(Clone, Debug)]
pub enum MarketMessage {
    Bybit(BybitMarket),
    Binance(BinanceMarket),
}

#[derive(Debug)]
pub enum Market {
    Bybit,
    Binance,
}

#[derive(Debug)]
pub enum ExchangeTrades {
    Bybit(WsTrade),
    Binance(AggrTradesEvent),
}

impl Clone for ExchangeTrades {
    fn clone(&self) -> Self {
        match self {
            Self::Bybit(v) => Self::Bybit(v.clone()),
            Self::Binance(v) => Self::Binance(v.clone()),
        }
    }
}
impl ExchangeTrades {
    fn unwrap_trade(&self) -> Option<WsTrade> {
        match self {
            ExchangeTrades::Bybit(v) => Some(v.clone()),
            ExchangeTrades::Binance(v) => Some(v.clone().process_trade()),
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
            price: self.price.parse().unwrap_or_else(|_| 0.0),
            volume: self.qty.parse().unwrap_or_else(|_| 0.0),
            side: self.event_type.clone(),
            tick_direction: "Zero".to_string(),
            id: self.aggregated_trade_id.to_string(),
            buyer_is_maker: self.is_buyer_maker,
        }
    }
}
