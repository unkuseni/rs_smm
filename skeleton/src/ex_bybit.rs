use bybit::{
    api::Bybit,
    model::{Category, OrderBookUpdate, Subscription, WebsocketEvents},
    ws::Stream as BybitStream,
};

use crate::util::{helpers::generate_timestamp, localorderbook::LocalOrderBook};

pub fn orderbook() {
    let stream: BybitStream = Bybit::new(None, None);
    let request = Subscription {
        args: vec!["orderbook.50.AIUSDT", "orderbook.1.AIUSDT"],
        op: "subscribe",
    };
    let mut localbook: LocalOrderBook = LocalOrderBook::new(50);
    let handler = move |event: WebsocketEvents| {
        match event {
            WebsocketEvents::OrderBookEvent(OrderBookUpdate { data, .. }) => {
                localbook.update(data.bids, data.asks, generate_timestamp());

            }
            _ => {}
        }
        println!("\nAsks: ");
         for (key, value) in localbook.asks.iter().rev().take(15) {
            println!("Price: {} =>  Qty: {}", key, value);
        }
        println!("\nBids: ");
        for (key, value) in localbook.bids.iter().rev().take(15) {
            println!("Price: {} => Qty: {}", key, value);
        }
        Ok(())
    };
    
    let _response = stream.ws_subscribe(request, Category::Linear, handler);
}
