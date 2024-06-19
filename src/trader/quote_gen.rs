use std::collections::VecDeque;

use binance::futures::account::FuturesAccount;
use bybit::trade::Trader;
use skeleton::{
    exchanges::exchange::{ExchangeClient, PrivateData},
    util::{
        helpers::{nbsqrt, round_step, Round},
        localorderbook::LocalBook,
    },
};
use tokio::sync::mpsc::UnboundedReceiver;

type BybitTrader = Trader;
type BinanceTrader = FuturesAccount;

enum OrderManagement {
    Bybit(BybitTrader),
    Binance(BinanceTrader),
}
pub struct QuoteGenerator {
    asset: f64,
    client: OrderManagement,
    rounding_value: usize,
    positions: f64,
    live_buys_orders: VecDeque<LiveOrder>,
    live_sells_orders: VecDeque<LiveOrder>,
    max_position_usd: f64,
    inventory_delta: f64,
}

impl QuoteGenerator {
    pub fn new(client: ExchangeClient, rounding_value: usize) -> Self {
        let trader = match client {
            ExchangeClient::Bybit(cl) => OrderManagement::Bybit(cl.bybit_trader()),
            ExchangeClient::Binance(cl) => OrderManagement::Binance(cl.binance_trader()),
        };
        QuoteGenerator {
            asset: 0.0,
            rounding_value,
            client: trader,
            positions: 0.0,
            live_buys_orders: VecDeque::new(),
            live_sells_orders: VecDeque::new(),
            inventory_delta: 0.0,
            max_position_usd: 0.0,
        }
    }

    fn update_max(&mut self) {
        self.max_position_usd = self.asset / 2.0;
    }
    pub fn start_loop(&mut self, mut receiver: UnboundedReceiver<PrivateData>) {}

    fn generate_quotes() {}

    fn total_orders(&self) -> usize {
        self.live_buys_orders.len() + self.live_sells_orders.len()
    }

    fn adjusted_skew(&self, mut skew: f64) -> f64 {
        let amount = nbsqrt(self.inventory_delta);
        skew += -amount;

        skew
    }

    fn adjusted_spread(&self, preferred_spread: f64, book: &LocalBook) -> f64 {
        let min_spread = bps_to_decimal(preferred_spread);
        book.get_spread()
            .clip(min_spread, min_spread * 3.7)
    }
    
}

#[derive(Debug, Clone)]
pub struct LiveOrder {
    pub price: f64,
    pub qty: f64,
    pub side: String,
    pub order_id: String,
}

pub fn inventory_delta(qty: f64, price: f64, max_position: f64) -> f64 {
    price * qty / max_position
}

fn max_position(book: &LocalBook, amount: f64) -> f64 {
    amount / book.mid_price
}

fn bps_to_decimal(bps: f64) -> f64 {
    bps / 10000.0
}

fn bps_offset(book: &LocalBook, bps: f64) -> f64 {
    book.mid_price + (book.mid_price * bps_to_decimal(bps))
}

fn offset(book: &LocalBook, offset: f64) -> f64 {
    book.mid_price + (book.mid_price * offset)
}

fn round_price(book: &LocalBook, price: f64) -> f64 {
    let val = book.tick_size.count_decimal_places();
    price.round_to(val as u8)
}

fn round_size(price: f64, step: f64) -> f64 {
    round_step(price, step)
}

pub fn liquidate_inventory() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inventory_delta() {
        let qty = 10.0;
        let price = 1.0;
        let max_position = 100.0;
        let delta = inventory_delta(qty, price, max_position);
        assert_eq!(delta, 0.10);
    }
}
