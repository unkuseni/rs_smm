use std::collections::HashMap;

use rs_smm::{parameters::parameters::use_toml, strategy::market_maker::MarketMaker};
use skeleton::ss;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    let config = use_toml();
    let mut state = ss::SharedState::new(config.exchange);
    let symbols: Vec<String> = {
        let mut arr = vec![];
        for v in config.symbols {
            arr.push(v);
        }
        arr
    };
    state.add_symbols(symbols);
    let clients = config.api_keys;
    for (key, secret, symbol) in clients {
        state.add_clients(key, secret, symbol, None);
    }
    let balance = {
        let mut new_map = HashMap::new();
        for (k, v) in config.balances {
            new_map.insert(k, v);
        }
        new_map
    };
    let mut market_maker = MarketMaker::new(
        state.clone(),
        balance,
        config.leverage,
        config.orders_per_side,
        config.final_order_distance,
        config.depths,
        config.rate_limit,
    );
    market_maker.set_spread_toml(config.bps);
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        ss::load_data(state, sender).await;
    });
    market_maker
        .start_loop(receiver, config.use_wmid, config.rate_limit)
        .await;
}
