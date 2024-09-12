use std::collections::HashMap;

use rs_smm::{parameters::parameters::use_toml, strategy::market_maker::MarketMaker};
use skeleton::ss;
use tokio::sync::mpsc;

// Start the program
#[tokio::main]
async fn main() {
    // Pull the contents of the config file
    let config = use_toml();
    // initialize shared state and pass in  exchange, clients, symbols
    let mut state = ss::SharedState::new(config.exchange);
    state.add_symbols(config.symbols);
    let clients = config.api_keys;
    for (key, secret, symbol) in clients {
        state.add_clients(key, secret, symbol, None);
    }

    // Create a hashmap for balances of each client/symbols
    let balance = balances(config.balances);

    // Initialize the market maker and set the initial state, balance, leverage, orders per side, final order distance, depths, and rate limit
    let mut market_maker = MarketMaker::new(
        state.clone(),
        balance,
        config.leverage,
        config.orders_per_side,
        config.final_order_distance,
        config.depths,
        config.rate_limit,
        config.tick_window,
    ).await;

    // sets the  base spread in bps for profit
    market_maker.set_spread_toml(config.bps);

    // create an unbounded channel
    let (sender, receiver) = mpsc::unbounded_channel();

    // loads up the shareed state and sends it across the channel
    tokio::spawn(async move {
        ss::load_data(state, sender).await;
    });

    // passes in the data receiver to the market maker and starts the loop
    market_maker.start_loop(receiver).await;
}

fn balances(arr: Vec<(String, f64)>) -> HashMap<String, f64> {
    let mut new_map = HashMap::new();
    for (k, v) in arr {
        new_map.insert(k, v);
    }
    new_map
}
