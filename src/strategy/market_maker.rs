use bybit::WsTrade;
use skeleton::exchanges::exchange::{Client, Exchange, PrivateData};
use skeleton::util::localorderbook::LocalBook;
use skeleton::{exchanges::exchange::MarketMessage, ss::SharedState};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::interval;

use crate::features::engine::Engine;
use crate::trader::quote_gen::QuoteGenerator;

pub struct MarketMaker {
    pub features: HashMap<String, Engine>,
    pub old_books: HashMap<String, Arc<LocalBook>>,
    pub old_trades: HashMap<String, Arc<VecDeque<WsTrade>>>,
    pub curr_trades: HashMap<String, Arc<VecDeque<WsTrade>>>,
    pub prev_avg_trade_price: HashMap<String, f64>,
    pub generators: HashMap<String, QuoteGenerator>,
    pub depths: Vec<usize>,
    pub tick_window: usize,
}

impl MarketMaker {
    /// Constructs a new `MarketMaker` instance.
    ///
    /// # Arguments
    ///
    /// * `ss` - The shared state containing information about the markets.
    /// * `assets` - The account balance per symbol.
    /// * `leverage` - The leverage to use for position sizing.
    /// * `orders_per_side` - The number of orders to place on each side.
    /// * `final_order_distance` - The distance of the final order from the mid price.
    /// * `depths` - The depths at which to calculate features.
    /// * `rate_limit` - The per-refresh batch-call limit.
    /// * `tick_window` - The feature lookback window in ticks.
    // The constructor mirrors the flat config fields; grouping them into a
    // struct would add a parallel type to keep in sync.
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        ss: SharedState,
        assets: HashMap<String, f64>,
        leverage: f64,
        orders_per_side: usize,
        final_order_distance: f64,
        depths: Vec<usize>,
        rate_limit: u32,
        tick_window: usize,
    ) -> Self {
        MarketMaker {
            features: MarketMaker::build_features(ss.symbols.clone(), tick_window),
            old_books: HashMap::new(),
            old_trades: HashMap::new(),
            curr_trades: HashMap::new(),
            prev_avg_trade_price: HashMap::new(),
            generators: MarketMaker::build_generators(
                ss.clients,
                assets,
                orders_per_side,
                leverage,
                final_order_distance,
                rate_limit,
            )
            .await,
            depths,
            tick_window,
        }
    }

    /// Continuously receives and processes shared state snapshots.
    ///
    /// The first `tick_window` messages are used only to warm up the features;
    /// after that the grid is (re)placed whenever needed.
    pub async fn start_loop(&mut self, mut receiver: UnboundedReceiver<Arc<SharedState>>) {
        let mut warmup_ticks = 0;
        let mut wait = interval(Duration::from_millis(30));
        let mut warned_both = false;
        while let Some(data) = receiver.recv().await {
            match data.exchange.as_str() {
                "bybit" | "binance" => {
                    // Update features with the latest market data.
                    self.update_features(data.markets[0].clone(), self.depths.clone());

                    // Replace the grid once the features are warmed up.
                    if warmup_ticks > self.tick_window {
                        self.potentially_update(data.private.clone(), data.markets[0].clone())
                            .await;
                    } else {
                        wait.tick().await;
                        warmup_ticks += 1;
                    }
                }
                "both" => {
                    if !warned_both {
                        eprintln!(
                            "'both' exchange mode is not yet supported by the strategy loop; \
                             no orders will be placed"
                        );
                        warned_both = true;
                    }
                }
                _ => {
                    panic!("Invalid exchange");
                }
            }
        }
    }

    /// Cancels all open orders and prints the final position per symbol.
    /// Called on graceful shutdown.
    pub async fn shutdown(&mut self) {
        for (symbol, generator) in self.generators.iter() {
            match generator.cancel_all(symbol).await {
                Ok(_) => println!("Cancelled all orders for {}", symbol),
                Err(_) => eprintln!("Failed to cancel all orders for {}", symbol),
            }
            println!("Final position for {}: {:.6}", symbol, generator.position);
        }
    }

    /// Builds a feature engine per symbol.
    fn build_features(symbol: Vec<String>, tick_window: usize) -> HashMap<String, Engine> {
        symbol
            .into_iter()
            .map(|v| (v, Engine::new(tick_window)))
            .collect()
    }

    /// Builds a quote generator per client symbol, setting leverage and
    /// syncing the position with the exchange before quoting starts.
    async fn build_generators(
        clients: HashMap<String, Client>,
        assets: HashMap<String, f64>,
        orders_per_side: usize,
        leverage: f64,
        final_order_distance: f64,
        rate_limit: u32,
    ) -> HashMap<String, QuoteGenerator> {
        let mut hash: HashMap<String, QuoteGenerator> = HashMap::new();
        let leverage = leverage.clamp(1.0, 100.0);

        for (symbol, client) in clients {
            // Missing balances produce zero-sized grids rather than panicking.
            let asset = assets.get(&symbol).copied().unwrap_or_else(|| {
                eprintln!(
                    "No balance configured for {}; position sizing will be zero",
                    symbol
                );
                0.0
            });

            match &client {
                Client::Bybit(cl) => match cl.set_leverage(&symbol, leverage as u16).await {
                    Ok(_) => println!("Set leverage for {} to {}", symbol, leverage),
                    Err(e) => eprintln!("Failed to set leverage for {}: {}", symbol, e),
                },
                Client::Binance(cl) => match cl.set_leverage(&symbol, leverage as u16).await {
                    Ok(_) => println!("Set leverage for {} to {}", symbol, leverage),
                    Err(e) => eprintln!("Failed to set leverage for {}: {}", symbol, e),
                },
            }

            let mut generator = QuoteGenerator::new(
                client,
                asset,
                leverage,
                orders_per_side,
                final_order_distance,
                rate_limit,
            );
            // Reconcile with the exchange's actual position before quoting.
            generator.sync_position(&symbol).await;
            hash.insert(symbol, generator);
        }

        hash
    }

    /// Updates the feature engines from the latest market message.
    fn update_features(&mut self, data: MarketMessage, depth: Vec<usize>) {
        // Both exchange variants share the same (symbol, Arc<book>)/(symbol, Arc<trades>) shape.
        let (trades, books) = match data {
            MarketMessage::Bybit(v) => (v.trades, v.books),
            MarketMessage::Binance(v) => (v.trades, v.books),
        };

        for (k, t) in trades {
            self.curr_trades.insert(k, t);
        }

        for (k, b) in books {
            let Some(feature) = self.features.get_mut(&k) else {
                eprintln!("No feature engine configured for symbol {}", k);
                continue;
            };

            let prev_book = self.old_books.get(&k);
            let prev_trade = self.old_trades.get(&k);
            let prev_avg = self.prev_avg_trade_price.get(&k);
            let curr_trade = self.curr_trades.get(&k);

            if let (Some(book), Some(p_trades), Some(p_avg), Some(curr_trades)) =
                (prev_book, prev_trade, prev_avg, curr_trade)
            {
                feature.update(
                    b.as_ref(),
                    book.as_ref(),
                    curr_trades.as_ref(),
                    p_trades.as_ref(),
                    p_avg,
                    depth.clone(),
                );
            }

            self.old_books.insert(k.clone(), b);
            self.prev_avg_trade_price.insert(k, feature.avg_trade_price);
        }

        // Cheap Arc clones; the underlying buffers are shared.
        self.old_trades = self.curr_trades.clone();
    }

    /// Updates each symbol's quote grid with new market and private data.
    async fn potentially_update(
        &mut self,
        private: HashMap<String, PrivateData>,
        data: MarketMessage,
    ) {
        let books = match data {
            MarketMessage::Bybit(v) => v.books,
            MarketMessage::Binance(v) => v.books,
        };

        for (symbol, book) in books {
            let Some(skew) = self.features.get(&symbol).map(|f| f.skew) else {
                eprintln!("No feature engine for {}; skipping update", symbol);
                continue;
            };
            let Some(symbol_quoter) = self.generators.get_mut(&symbol) else {
                eprintln!("No quote generator for {}; skipping update", symbol);
                continue;
            };

            if let Some(private_data) = private.get(&symbol) {
                symbol_quoter
                    .update_grid(private_data.clone(), skew, (*book).clone(), symbol)
                    .await;
            }
        }
    }

    /// Sets the spread per generator from a symbol-keyed map of basis points.
    pub fn set_spread_toml(&mut self, bps: HashMap<String, f64>) {
        for (symbol, generator) in self.generators.iter_mut() {
            match bps.get(symbol) {
                Some(spread) => generator.set_spread(*spread),
                None => eprintln!(
                    "No spread configured for {}; using the built-in default",
                    symbol
                ),
            }
        }
    }
}
