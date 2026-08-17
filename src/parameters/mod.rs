use skeleton::util::helpers::{read_toml, Config};

/// Loads the configuration from `./config.toml`, or from the path given by
/// the `RS_SMM_CONFIG` environment variable when set.
pub fn use_toml() -> Config {
    let path = std::env::var("RS_SMM_CONFIG").unwrap_or_else(|_| "./config.toml".to_string());
    read_toml(path)
}
