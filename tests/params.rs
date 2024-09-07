
#[cfg(test)]
mod tests {
    use rs_smm::parameters::parameters::{exch_params, use_toml};

    #[test]
    fn test_exch_params() {
        let exch = exch_params();
        assert_eq!(exch, "bybit");
    }

    #[test]
    fn test_toml() {
        let config = use_toml();
        assert_eq!(config.exchange, "bybit");
        println!("{:#?}", config);
    }
}
