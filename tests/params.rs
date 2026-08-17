#[cfg(test)]
mod tests {
    use rs_smm::parameters::use_toml;

    #[test]
    fn test_toml() {
        let config = use_toml();
        assert_eq!(config.exchange, "bybit");
        println!("{:#?}", config);
    }
}
