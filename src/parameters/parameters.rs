use std::{
    collections::HashMap,
    io::{self},
};

use skeleton::util::helpers::{read_toml, Config};

pub fn watch(prompt: &str) -> String {
    println!("{}", prompt);
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read input");
    input.trim().to_string()
}

pub fn exch_params() -> String {
    let prompt = "Available Exchanges are \"bybit\" | \"binance\" | \"both\"  \n Functionality for \"both\" or \"binance\" is unstable \n Please select an exchange: ";
    let exch = watch(prompt);
    println!("Selected Exchange: {}", exch);
    exch
}

pub fn symbol_params() -> Vec<String> {
    let mut symbol_arr = vec![];
    loop {
        let prompt = "Markets to Make!!! \nPlease enter a symbol: ";
        let symbol = watch(prompt);
        if symbol == "" {
            break;
        }
        symbol_arr.push(symbol);
        println!(
            "Added Symbol: {:#?} To stop the loop leave symbol blank",
            symbol_arr
        );
    }
    symbol_arr
}

pub fn api_key_params() -> HashMap<String, (String, String)> {
    let mut api_keys = HashMap::new();
    loop {
        let prompt = "Please enter a symbol for Account Keys:";
        let symbol = watch(prompt).to_uppercase();
        if symbol == "" {
            break;
        }
        let prompt = format!("Please enter your API Key for {}: ", symbol.clone());
        let api_key = watch(&prompt);
        if api_key == "" {
            break;
        }
        let prompt = format!("Please enter your Secret Key for {}: ", symbol.clone());
        let secret_key = watch(&prompt);
        if secret_key == "" {
            break;
        }
        api_keys.insert(symbol.clone(), (api_key, secret_key));
        println!(
            "Added API Key for Symbol: {:?} \n To stop the loop leave symbol blank",
            symbol
        );
    }
    api_keys
}

pub fn acct_balance_params() -> HashMap<String, f64> {
    let mut balances = HashMap::new();
    loop {
        let prompt = "Please enter a symbol for account balances:";
        let symbol = watch(prompt).to_uppercase();
        if symbol == "" {
            break;
        }
        let prompt = format!("Please enter your balance for {}: ", symbol.clone());
        let balance = watch(&prompt);
        if balance == "" {
            break;
        }
        balances.insert(symbol.clone(), balance.parse::<f64>().unwrap());
        println!(
            "Added Balance for Symbol: {} \n To stop the loop leave symbol blank",
            symbol
        );
    }
    balances
}

pub fn maker_params() -> MakerParams {
    let leverage = watch("This section applies to all quote generators. \nExample: 10. \n Note: Account leverage must be set for symbol to be used, that functionality will soon be added. \n Please enter leverage: ")
        .parse::<f64>()
        .unwrap();
    let orders_per_side = watch("Example: 5. Please enter orders per side: ")
        .parse::<usize>()
        .unwrap();
    let final_order_distance = watch("This is a multiplier for the quote spread Eg 10 or 7.5. Please enter final order distance: ").parse::<f64>().unwrap();
    let depths = watch("Example: 5,50. \n Please enter depths: ")
        .split(',')
        .map(|x| x.parse::<usize>().unwrap())
        .collect();
    let rebalance_ratio = watch("Parameter for rebalancing book if inventory is greater than that. Please enter rebalance ratio: ").parse::<f64>().unwrap();
    let rate_limit = watch("Parameter for rate limit. Please enter rate limit: ")
        .parse::<u32>()
        .unwrap();
    let params = MakerParams::new(
        leverage,
        orders_per_side,
        final_order_distance,
        depths,
        rebalance_ratio,
        rate_limit,
    );
    params
}

pub fn use_toml() -> Config {
    let path = "./config.toml";
    let result = read_toml(path);
    result
}

pub struct MakerParams {
    pub leverage: f64,
    pub orders_per_side: usize,
    pub final_order_distance: f64,
    pub depths: Vec<usize>,
    pub rebalance_ratio: f64,
    pub rate_limit: u32,
}

impl MakerParams {
    pub fn new(
        leverage: f64,
        orders_per_side: usize,
        final_order_distance: f64,
        depths: Vec<usize>,
        rebalance_ratio: f64,
        rate_limit: u32,
    ) -> Self {
        Self {
            leverage,
            orders_per_side,
            final_order_distance,
            depths,
            rebalance_ratio,
            rate_limit,
        }
    }
}
