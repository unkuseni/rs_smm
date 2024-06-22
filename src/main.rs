use rs_smm::strategy::market_maker::MarketMaker;
use skeleton::ss;
use std::collections::HashMap;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    let mut state = ss::SharedState::new("bybit");
    state.add_symbols(["NOTUSDT"].to_vec());
    state.add_clients("45wk2c8nYInQC6sUSy".to_string(), "QgOdljngpGqyoGUp5AYZOLWG84Prm2T1fJ2n".to_string(), "NOTUSDT".to_string(), Some("bybit"));
    let mut market_maker = MarketMaker::new(state.clone(), assets());
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        ss::load_data(state, sender).await;
    });
    market_maker.start_loop(receiver).await;
}

fn assets() -> HashMap<String, f64> {
    let mut map = HashMap::new();
    map.insert("NOTUSDT".to_string(), 100.0);
    map
}
