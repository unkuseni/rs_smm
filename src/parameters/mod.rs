use skeleton::util::helpers::{read_toml, Config};

/// The path configuration is read from: `./config.toml`, or the path given
/// by the `RS_SMM_CONFIG` environment variable when set.
pub fn config_path() -> String {
    std::env::var("RS_SMM_CONFIG").unwrap_or_else(|_| "./config.toml".to_string())
}

/// Loads the configuration from `./config.toml`, or from the path given by
/// the `RS_SMM_CONFIG` environment variable when set.
///
/// Rejects unsupported configurations at parse time so they fail fast instead
/// of running a no-op strategy loop.
pub fn use_toml() -> Config {
    let path = config_path();
    let config = read_toml(path);
    if config.exchange == "both" {
        panic!(
            "exchange = \"both\" is not supported by the strategy loop yet; \
             use \"bybit\" or \"binance\""
        );
    }
    if config.orders_per_side == 0 {
        panic!("orders_per_side must be at least 1");
    }
    if config.depths.is_empty() || config.depths.contains(&0) {
        panic!("depths must be a non-empty list of positive levels");
    }
    if config.final_order_distance <= 0.0 {
        panic!("final_order_distance must be positive");
    }
    if config.leverage <= 0.0 {
        panic!("leverage must be positive (it is later clamped to 1..=100)");
    }
    config
}
