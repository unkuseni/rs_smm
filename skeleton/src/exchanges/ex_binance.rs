use std::sync::atomic::AtomicBool;

use binance::account::Account;
use binance::futures::account::{self, FuturesAccount};
use binance::futures::model::AccountBalance;
use binance::model::AccountInformation;
use binance::{api::Binance, futures::websockets::*, general::General, model::AggrTradesEvent};
use tokio::sync::mpsc;

use super::ex_bybit::{BybitMarket, BybitPrivate};
use super::exchange::Exchange;
#[derive(Clone, Debug, PartialEq)]
pub struct BinanceClient {
    pub key: String,
    pub secret: String,
}

impl BinanceClient {
    pub fn init(key: String, secret: String) -> Self {
        Self { key, secret }
    }
    pub async fn exchange_time(&self) -> u64 {
        let response = tokio::task::spawn_blocking(|| {
            let general: General = Binance::new(None, None);
            general.get_server_time()
        })
        .await
        .expect("Failed to get server time");
        if let Ok(v) = response {
            println!("Server time: {}", v.server_time);
            v.server_time
        } else {
            0
        }
    }

    pub async fn market_subscribe(
        &self,
        symbol: Vec<&str>,
        sender: mpsc::UnboundedSender<BybitMarket>,
    ) {
        unimplemented!()
    }

    pub async fn private_subscribe(&self, _sender: mpsc::UnboundedSender<BybitPrivate>) {
        unimplemented!()
    }

    pub async fn fee_rate(&self, _symbol: &str) -> f64 {
        unimplemented!()
    }

    pub fn fee_rate2(&self, _symbol: &str) -> Vec<AccountBalance> {
        let client: FuturesAccount =
            Binance::new(Some(self.key.clone()), Some(self.secret.clone()));

        let response = client.account_balance();
        if let Ok(v) = response {
            println!("Fee rate: {:#?}", v);
            v
        } else {
            unimplemented!()
        }
    }
    pub fn ws_subscribe(&self, sender: mpsc::UnboundedSender<FuturesWebsocketEvent>) {
        let keep_running = AtomicBool::new(true); // Used to control the event loop
        let agg_trade = String::from("ethbtc@aggTrade");
        let mut web_socket: FuturesWebSockets<'_> =
            FuturesWebSockets::new(|event: FuturesWebsocketEvent| {
                match event {
                    FuturesWebsocketEvent::Trade(trade) => {
                        sender
                            .send(FuturesWebsocketEvent::Trade(trade.clone()))
                            .unwrap();
                        println!(
                            "Symbol: {}, price: {}, qty: {}",
                            trade.symbol, trade.price, trade.qty
                        );
                    }
                    FuturesWebsocketEvent::DepthOrderBook(depth_order_book) => {
                        sender
                            .send(FuturesWebsocketEvent::DepthOrderBook(
                                depth_order_book.clone(),
                            ))
                            .unwrap();
                        println!(
                            "Symbol: {}, Bids: {:?}, Ask: {:?}",
                            depth_order_book.symbol, depth_order_book.bids, depth_order_book.asks
                        );
                    }
                    FuturesWebsocketEvent::OrderBook(order_book) => {
                        sender
                            .send(FuturesWebsocketEvent::OrderBook(order_book.clone()))
                            .unwrap();
                        println!(
                            "last_update_id: {}, Bids: {:?}, Ask: {:?}",
                            order_book.last_update_id, order_book.bids, order_book.asks
                        );
                    }
                    _ => (),
                };

                Ok(())
            });

        web_socket
            .connect(&FuturesMarket::USDM, &agg_trade)
            .unwrap(); // check error
        if let Err(e) = web_socket.event_loop(&keep_running) {
            println!("Error: {}", e);
        }
    }
    pub fn ws_aggtrades(&self, symbol: Vec<&str>, sender: mpsc::UnboundedSender<AggrTradesEvent>) {
        let keep_running = AtomicBool::new(true); // Used to control the event loop
        let agg_trades: Vec<String> = symbol
            .iter()
            .map(|&sub| sub.to_lowercase())
            .map(|sub| format!("{}@aggTrade", sub))
            .collect();
        let mut web_socket = FuturesWebSockets::new(|event: FuturesWebsocketEvent| {
            if let FuturesWebsocketEvent::AggrTrades(agg) = event {
                // println!(
                //     "Symbol: {}, price: {}, qty: {}",
                //     agg.symbol, agg.price, agg.qty
                // );
                sender.send(agg).unwrap();
            } else {
                println!("Unexpected event: {:?}", event);
            }

            Ok(())
        });
        web_socket
            .connect_multiple_streams(&FuturesMarket::USDM, &agg_trades)
            .unwrap();

        // check error
        if let Err(e) = web_socket.event_loop(&keep_running) {
            println!("Error: {}", e);
        }
    }
}
