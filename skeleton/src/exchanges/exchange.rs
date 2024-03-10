use super::{ex_binance::BinanceClient, ex_bybit::BybitClient};



#[derive(Clone, Debug, PartialEq)]
pub enum Exchange {
    Bybit(BybitClient),
    Binance(BinanceClient),
}
