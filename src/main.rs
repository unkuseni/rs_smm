use std::collections::HashMap;

use rs_smm::{parameters::parameters::use_toml, strategy::market_maker::MarketMaker};
use skeleton::{ss, util::helpers::Config};
use tokio::sync::mpsc;

// Start the program
#[tokio::main]
async fn main() {
    // Pull the contents of the config file
    let Config {
        exchange,
        symbols,
        api_keys,
        balances,
        leverage,
        orders_per_side,
        final_order_distance,
        depths,
        rate_limit,
        tick_window,
        bps,
    } = use_toml();
    // initialize shared state and pass in  exchange, clients, symbols
    let mut state = ss::SharedState::new(exchange);
    state.add_symbols(symbols);
    let clients = api_keys;
    for (key, secret, symbol) in clients {
        state.add_clients(key, secret, symbol, None);
    }

    // Create a hashmap for balances of each client/symbols
    let balance = map_balances(balances);

    // Initialize the market maker and set the initial state, balance, leverage, orders per side, final order distance, depths, and rate limit
    let mut market_maker = MarketMaker::new(
        state.clone(),
        balance,
        leverage,
        orders_per_side,
        final_order_distance,
        depths,
        rate_limit,
        tick_window,
    )
    .await;

    // sets the  base spread in bps for profit
    market_maker.set_spread_toml(bps);

    // create an unbounded channel
    let (sender, receiver) = mpsc::unbounded_channel();

    // loads up the shareed state and sends it across the channel
    tokio::spawn(async move {
        ss::load_data(state, sender).await;
    });

    // passes in the data receiver to the market maker and starts the loop
    market_maker.start_loop(receiver).await;
}

fn map_balances(arr: Vec<(String, f64)>) -> HashMap<String, f64> {
    let mut new_map = HashMap::new();
    for (k, v) in arr {
        new_map.insert(k, v);
    }
    new_map
}
