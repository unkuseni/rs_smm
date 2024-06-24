use std::{
    collections::HashMap,
    io::{self, Write},
};

use crate::strategy::market_maker::MarketMaker;

fn watch(prompt: &str) -> String {
    println!("{}", prompt);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("failed to read input");
    let received: String = input.trim().to_string().parse().expect("failed to parse");
    received
}

fn watch_static(prompt: &str) -> &'static str {
    println!("{}", prompt);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("failed to read input");
    let received: &'static str = Box::leak(input.trim().to_string().into_boxed_str());
    received
}

pub fn exch_params() -> &'static str {
    let prompt = "Available Exchanges are \"bybit\" | \"binance\" | \"both\"  \n Functionality for \"both\" or \"binance\" is unstable \n Please select an exchange: ";
    let exch = watch_static(prompt);
    println!("Selected Exchange: {}", exch);
    exch
}

pub fn symbol_params() -> Vec<&'static str> {
    let mut symbol_arr = vec![];
    loop {
        let prompt = "Markets to Make!!! \nPlease enter a symbol: ";
        let symbol = watch_static(prompt);
        if symbol == "" {
            break;
        }
        symbol_arr.push(symbol);
        println!("Added Symbol: {:#?} To stop the loop leave symbol blank", symbol_arr);
    }
    symbol_arr
}

pub fn api_key_params() -> HashMap<String, (String, String)> {
    let mut api_keys = HashMap::new();
    loop {
        let prompt = "Please enter a symbol for Account Keys:";
        let symbol = watch(prompt);
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
        let symbol = watch(prompt);
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
    let interval = watch("Example: 1000. \n Converts to milliseconds. Please enter interval: ")
        .parse::<u64>()
        .unwrap();
    let depths = watch("Example: 5,50. \n Please enter depths: ")
        .split(',')
        .map(|x| x.parse::<usize>().unwrap())
        .collect();
    let rebalance_ratio = watch("Parameter for rebalancing book if inventory is greater than that. Please enter rebalance ratio: ").parse::<f64>().unwrap();
    let params = MakerParams::new(
        leverage,
        orders_per_side,
        final_order_distance,
        interval,
        depths,
        rebalance_ratio
    );
    params
}
impl MarketMaker {
    pub fn set_spread_bps(&mut self) {
        for (k, v) in self.generators.iter_mut() {
            let prompt = format!("Please enter spread for {} in bps: ", k);
            let spread = watch(&prompt).parse::<f64>().unwrap();
            v.set_spread(spread);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exch_params() {
        let exch = exch_params();
        assert_eq!(exch, "bybit");
    }
}

pub struct MakerParams {
    pub leverage: f64,
    pub orders_per_side: usize,
    pub final_order_distance: f64,
    pub interval: u64,
    pub depths: Vec<usize>,
    pub rebalance_ratio: f64
}

impl MakerParams {
    pub fn new(
        leverage: f64,
        orders_per_side: usize,
        final_order_distance: f64,
        interval: u64,
        depths: Vec<usize>,
        rebalance_ratio: f64
    ) -> Self {
        Self {
            leverage,
            orders_per_side,
            final_order_distance,
            interval,
            depths,
            rebalance_ratio
        }
    }
}
