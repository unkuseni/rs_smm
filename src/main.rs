use std::collections::HashMap;
use std::sync::Arc;

use rs_smm::{parameters::use_toml, strategy::market_maker::MarketMaker};
use skeleton::{ss, util::helpers::Config};
use tokio::sync::mpsc;

// Start the program
#[tokio::main]
async fn main() {
    // Pull the contents of the config file (path overridable via RS_SMM_CONFIG).
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

    // Initialize shared state with the exchange, clients, and symbols.
    let mut state = ss::SharedState::new(exchange);
    state.add_symbols(symbols);
    for (key, secret, symbol) in api_keys {
        // A value prefixed with "env:" is resolved from the environment so
        // credentials never have to live in the config file.
        let key = resolve_secret(key);
        let secret = resolve_secret(secret);
        state.add_clients(key, secret, symbol, None);
    }

    // Create a hashmap for the balances of each client/symbol.
    let balance = map_balances(balances);

    // Initialize the market maker with the initial state, balance, leverage,
    // orders per side, final order distance, depths, and rate limit.
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

    // Set the base spread in bps for profit, keyed by symbol.
    market_maker.set_spread_toml(bps);

    // Create a channel for shared-state snapshots.
    let (sender, receiver) = mpsc::unbounded_channel::<Arc<ss::SharedState>>();

    // Load market/private data and send snapshots across the channel.
    tokio::spawn(async move {
        ss::load_data(state, sender).await;
    });

    // Run the strategy loop until it ends or the user interrupts (Ctrl+C),
    // then cancel all open orders before exiting.
    tokio::select! {
        _ = market_maker.start_loop(receiver) => {}
        _ = tokio::signal::ctrl_c() => {
            println!("Received Ctrl+C; shutting down");
        }
    }
    market_maker.shutdown().await;
}

/// Resolves a config value that references an environment variable
/// (`env:VAR_NAME`) or returns the value unchanged.
fn resolve_secret(value: String) -> String {
    match value.strip_prefix("env:") {
        Some(var) => {
            std::env::var(var).unwrap_or_else(|_| panic!("Environment variable {} is not set", var))
        }
        None => value,
    }
}

fn map_balances(arr: Vec<(String, f64)>) -> HashMap<String, f64> {
    arr.into_iter().collect()
}
