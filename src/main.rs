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
        record,
        state_file,
        strategy,
        turso,
    } = use_toml();

    // Initialize shared state with the exchange, clients, and symbols.
    let mut state = ss::SharedState::new(exchange);
    // Optional websocket-delta recording for offline replay.
    state.record = record.clone();
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
        strategy,
    )
    .await;

    // Set the base spread in bps for profit, keyed by symbol.
    market_maker.set_spread_toml(bps);

    // Optional per-symbol state persistence across restarts.
    market_maker.state_file = state_file;
    if let Err(e) = market_maker.load_state() {
        eprintln!("Failed to restore state: {}", e);
    }

    // Optional Turso (libSQL) telemetry database. Connection failures are
    // non-fatal: the bot continues without telemetry.
    if let Some(url) = turso.url {
        let token = turso.auth_token.unwrap_or_default();
        let url = resolve_secret(url);
        let token = resolve_secret(token);
        match rs_smm::db::TursoDb::connect(&url, &token).await {
            Ok(db) => {
                if let Err(e) = db.init().await {
                    eprintln!("Turso schema init failed: {}", e);
                }
                market_maker.db_sync_ms = turso.sync_interval_secs.saturating_mul(1000);
                market_maker.db = Some(db);
                println!("Turso telemetry connected");
            }
            Err(e) => eprintln!("Turso connection failed (continuing without database): {}", e),
        }
    }

    // Create a channel for shared-state snapshots.
    let (sender, receiver) = mpsc::unbounded_channel::<Arc<ss::SharedState>>();

    // Watch the config file and push reloads to the strategy loop.
    let (config_sender, config_receiver) = mpsc::unbounded_channel::<Config>();
    {
        let config_path = rs_smm::parameters::config_path();
        tokio::spawn(async move {
            if let Err(e) = skeleton::util::helpers::watch_config(
                config_path,
                std::time::Duration::from_secs(5),
                config_sender,
            )
            .await
            {
                eprintln!("Config watcher stopped: {}", e);
            }
        });
    }

    // Load market/private data and send snapshots across the channel.
    tokio::spawn(async move {
        ss::load_data(state, sender, record).await;
    });

    // Run the strategy loop until it ends or the user interrupts (Ctrl+C),
    // then cancel all open orders before exiting.
    tokio::select! {
        _ = market_maker.start_loop(receiver, config_receiver) => {}
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
