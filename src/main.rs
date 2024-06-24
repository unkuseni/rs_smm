use rs_smm::parameters::parameters::{
    acct_balance_params, api_key_params, exch_params as ex_input, maker_params,
    symbol_params as symbol_input, MakerParams,
};
use rs_smm::strategy::market_maker::MarketMaker;
use skeleton::ss;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    let mut state = ss::SharedState::new(ex_input());
    state.add_symbols(symbol_input());
    let clients = api_key_params();
    for (k, v) in clients {
        state.add_clients(v.0, v.1, k, None);
    }
    let balance = acct_balance_params();
    let MakerParams {
        leverage,
        orders_per_side,
        final_order_distance,
        interval,
        depths,
        rebalance_ratio,
    } = maker_params();
    let mut market_maker = MarketMaker::new(
        state.clone(),
        balance,
        leverage,
        orders_per_side,
        final_order_distance,
        interval,
        depths,
        rebalance_ratio,
    );
    market_maker.set_spread_bps();
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        ss::load_data(state, sender).await;
    });
    market_maker.start_loop(receiver).await;
}
