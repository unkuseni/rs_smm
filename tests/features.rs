#[cfg(test)]
mod tests {

    use std::collections::HashMap;

    use ndarray::{array, Array1, Array2};
    use rs_smm::{
        features::{
            imbalance::{calculate_ofi, imbalance_ratio, voi},
            impact::{expected_return, expected_value, mid_price_change, price_flu},
            linear_reg::mid_price_regression,
        },
        parameters::parameters::use_toml,
    };
    use skeleton::{
        exchanges::exchange::MarketMessage,
        ss::{self, SharedState},
        util::localorderbook::LocalBook,
    };
    use tokio::sync::mpsc::{self, UnboundedReceiver};

    #[test]
    fn test() {
        println!("test");
    }

    #[tokio::test]
    async fn test_imbalance() {
        let mut receiver = setup();

        while let Some(v) = receiver.recv().await {
            let market = &v.markets[0];
            match market {
                MarketMessage::Bybit(m) => {
                    let books = &m.books;
                    for b in books {
                        let symbol = &b.0;
                        let depth = 5;
                        println!(
                            "{} IMBALANCE AT DEPTH {}: {:.5} {:.7}",
                            symbol,
                            depth,
                            imbalance_ratio(&b.1, Some(depth)),
                            &b.1.get_mid_price()
                        );
                    }
                }
                _ => {}
            }
        }
    }

    #[tokio::test]
    async fn test_wmid() {
        let mut receiver = setup();

        while let Some(v) = receiver.recv().await {
            let market = &v.markets[0];
            match market {
                MarketMessage::Bybit(m) => {
                    let books = &m.books;
                    for b in books {
                        let symbol = &b.0;
                        let depth = 5;
                        println!(
                            "{} W-MID AT DEPTH {}: {:.6}",
                            symbol,
                            depth,
                            b.1.get_wmid(imbalance_ratio(&b.1, Some(depth)))
                        );
                    }
                }
                _ => {}
            }
        }
    }

    #[test]
    fn test_skew() {
        let skew: f64 = 0.70;
        let delta: f64 = -0.37;
        let sq_corrected = skew * (1.0 - delta.abs().sqrt());
        println!(
            "skew: {:.5} delta: {:.5} sq_corrected: {:.5}",
            skew, delta, sq_corrected
        );
    }

    #[tokio::test]
    async fn test_price() {
        let mut receiver = setup();

        while let Some(v) = receiver.recv().await {
            let market = &v.markets[0];
            match market {
                MarketMessage::Bybit(m) => {
                    let books = &m.books;
                    for b in books {
                        let symbol = &b.0;
                        let depth = 3;
                        println!(
                            "{} MID PRICE AT BBA: {:.6} \nMICROPRICE:  {:.6}  pred: {} \nWMID: {:.6}",
                            symbol,
                            b.1.get_mid_price(),
                            b.1.get_microprice(Some(depth)),
                            if  b.1.get_microprice(Some(depth)) - b.1.best_bid.price >
                           (b.1.get_microprice(Some(depth)) - b.1.best_ask.price).abs() { "BUY" } else  if  b.1.get_microprice(Some(depth)) - b.1.best_bid.price <
                           (b.1.get_microprice(Some(depth)) - b.1.best_ask.price).abs() { "SELL" } else { "HOLD" },
                            b.1.get_wmid(imbalance_ratio(&b.1, Some(depth)))
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn setup() -> UnboundedReceiver<ss::SharedState> {
        let config = use_toml();
        let mut state = SharedState::new(config.exchange);
        state.add_symbols(config.symbols);
        for (key, secret, symbol) in config.api_keys {
            state.add_clients(key, secret, symbol, None);
        }

        let (state_sender, receiver) = mpsc::unbounded_channel::<ss::SharedState>();
        tokio::spawn(async move {
            ss::load_data(state, state_sender).await;
        });
        receiver
    }

    #[tokio::test]
    async fn test_def_reg() {
        let mut receiver = setup();
        let mut tick = 0;
        let mut mid_prices = HashMap::new();
        let mut old_book = HashMap::new();
        let mut features = HashMap::new();
        for v in use_toml().symbols {
            features.insert(v.clone(), Vec::new());
            old_book.insert(v.clone(), LocalBook::new());
            mid_prices.insert(v, Vec::new());
        }
        while let Some(v) = receiver.recv().await {
            let market = &v.markets[0];
            match market {
                MarketMessage::Bybit(m) => {
                    let books = &m.books;
                    for b in books {
                        let symbol = &b.0;
                        let depth = 3;

                        if tick > 0 {
                            mid_prices.get_mut(symbol).unwrap().push(b.1.mid_price);
                            features.get_mut(symbol).unwrap().push(vec![
                                imbalance_ratio(&b.1, Some(depth)),
                                voi(&b.1, &old_book.get(symbol).unwrap(), Some(depth)),
                                calculate_ofi(&b.1, &old_book.get(symbol).unwrap(), Some(depth)),
                            ]);

                            println!(
                                "{} W-MID AT DEPTH {}: {:.6}",
                                symbol,
                                depth,
                                b.1.get_microprice(Some(depth))
                            );
                            if features.len() > 610 {
                                let mids = mid_prices.get(symbol).unwrap().clone();
                                let y = Array1::from_vec(mids);
                                match Array2::from_shape_vec(
                                    (features.get(symbol).unwrap().len(), 3),
                                    features
                                        .get(symbol)
                                        .unwrap()
                                        .clone()
                                        .into_iter()
                                        .flat_map(|v| v.into_iter())
                                        .collect::<Vec<f64>>(),
                                ) {
                                    Ok(x) => {
                                        println!(
                                            "{}: {:.6}",
                                            symbol,
                                            mid_price_regression(
                                                y,
                                                x,
                                                b.1.get_spread_in_bps() as f64
                                            )
                                            .unwrap_or(0.0)
                                        );
                                    }
                                    Err(_) => {}
                                };
                            }
                            if features.get(symbol).unwrap().len() > 987 {
                                for _ in 0..210 {
                                    features.get_mut(symbol).unwrap().remove(0);
                                    mid_prices.get_mut(symbol).unwrap().remove(0);
                                }
                            }
                        } else {
                            tick += 1;
                        }
                        old_book.insert(symbol.to_string(), b.1.clone());
                    }
                }
                _ => {}
            }
        }
    }

    #[test]
    fn test_deep() {
        let v = vec![
            -1.0, -0.8, -0.6, -0.4, -0.2, 0.0, 0.2, 0.4, 0.6, 0.8, 1.0, 0.99, 0.50, -0.23, 0.81,
            0.94,
        ];
        let norm = v.iter().sum::<f64>() / v.len() as f64;
        println!("Value: {:#?}", norm);
    }

    #[test]
    fn test_future_value() {
        let value = vec![
            expected_value(0.4076, 0.4079, 0.66),
            expected_value(0.4076, 0.4079, -0.66),
            expected_value(0.4076, 0.4079, 0.0),
            expected_value(0.4076, 0.4076, 0.80),
            expected_value(0.4076, 0.4076, -0.80),
            expected_value(0.4076, 0.4079, 0.80),
            expected_value(0.4076, 0.4079, -0.80),
            expected_value(0.4079, 0.4076, 0.80),
            expected_value(0.4079, 0.4076, -0.80),
        ];
        println!("Value: {:#?}", value);
    }

    #[test]
    fn test_mid_change() {
        let value = vec![
            mid_price_change(0.0012567, 0.0012572, 0.0000001),
            mid_price_change(0.0012572, 0.0012567, 0.0000001),
            mid_price_change(0.0012572, 0.0012586, 0.000001),
            mid_price_change(0.0012582, 0.0012573, 0.000001),
        ];
        println!("Value: {:#?}", value);
    }

    #[test]
    fn test_ret() {
        let value = vec![
            expected_return(0.001234, 0.001239),
            expected_return(0.001239, 0.001234),
        ];
        println!("Value: {:#?}", value);
    }

    #[test]
    fn test_flu() {
        let value = vec![price_flu(0.001234, 0.001239), price_flu(0.001239, 0.001234)];
        println!("Value: {:#?}", value);
    }

    #[test]
    fn test_mid_price_regression() {
        let mid_price = array![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let features = array![
            [1.0, 2.0, 3.0],
            [1.1, 2.2, 2.9],
            [1.2, 2.1, 3.1],
            [1.3, 2.3, 2.8],
            [1.4, 2.4, 3.2],
            [1.5, 2.5, 3.3],
            [1.6, 2.6, 3.4],
            [1.7, 2.7, 3.5],
            [1.8, 2.8, 3.6],
            [1.9, 2.9, 3.7]
        ];
        let curr_spread = 2.0;
        let result = mid_price_regression(mid_price, features, curr_spread).unwrap();
        println!("Result: {}", result);
        assert!((result - 5.5).abs() < 1e-6);
    }

    #[test]
    fn test_flatten() {
        let data = vec![[2, 2, 2], [3, 3, 3], [4, 4, 4], [5, 5, 5], [6, 6, 6]];
        let result = match Array2::from_shape_vec(
            (data.len(), data[0].len()),
            data.into_iter()
                .flat_map(|v| v.into_iter())
                .collect::<Vec<i32>>(),
        ) {
            Ok(x) => x,
            Err(e) => panic!("{}", e),
        };
        print!("Result: {:#?}", result.view());
    }

    #[test]
    fn test_mid_price_regression_extended() {
        let mid_price = array![
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0,
            17.0, 18.0, 19.0, 20.0
        ];

        let features = array![
            [1.0, 2.0, 3.0],
            [1.1, 2.2, 2.9],
            [1.2, 2.1, 3.1],
            [1.3, 2.3, 2.8],
            [1.4, 2.4, 3.2],
            [1.5, 2.5, 3.3],
            [1.6, 2.6, 3.4],
            [1.7, 2.7, 3.5],
            [1.8, 2.8, 3.6],
            [1.9, 2.9, 3.7],
            [2.0, 3.0, 3.8],
            [2.1, 3.1, 3.9],
            [2.2, 3.2, 4.0],
            [2.3, 3.3, 4.1],
            [2.4, 3.4, 4.2],
            [2.5, 3.5, 4.3],
            [2.6, 3.6, 4.4],
            [2.7, 3.7, 4.5],
            [2.8, 3.8, 4.6],
            [2.9, 3.9, 4.7]
        ];

        let curr_spread = 2.5;
        let result = mid_price_regression(mid_price, features, curr_spread).unwrap();
        println!("Result: {}", result);
        assert!((result - 10.5).abs() < 1e-6);
    }

    #[test]
    fn test_mid_price_regression_with_negatives() {
        let mid_price = array![-1.0, -0.5, 0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5];

        let features = array![
            [-1.0, -0.5, 0.0],
            [-0.8, -0.3, 0.2],
            [-0.6, -0.1, 0.4],
            [-0.4, 0.1, 0.6],
            [-0.2, 0.3, 0.8],
            [0.0, 0.5, 1.0],
            [0.2, 0.7, 1.2],
            [0.4, 0.9, 1.4],
            [0.6, 1.1, 1.6],
            [0.8, 1.3, 1.8]
        ];

        let curr_spread = 1.0;
        let result = mid_price_regression(mid_price, features, curr_spread).unwrap();
        println!("Result: {}", result);
        // Adjust this assertion based on the expected result
        assert!((result - 1.25).abs() < 1e-6);
    }

    #[test]
    fn test_mid_price_regression_with_negatives_extended() {
        // Add more negative values
        let mid_price = array![
            -1.0, -0.5, 0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 0.0, 5.0, 5.5, 6.0, 0.0, 7.0,
            7.5, 8.0, 8.5
        ];
        // Add more features
        let features = array![
            [1.0, -0.8, 0.0],
            [0.8, -0.3, 0.2],
            [0.6, 0.6, 0.4],
            [0.4, -0.1, 0.6],
            [0.2, -0.3, 0.8],
            [0.0, -0.5, 1.0],
            [0.2, -0.7, 1.2],
            [0.4, 0.9, 1.4],
            [0.6, -1.1, 1.6],
            [0.8, 0.1, 1.8],
            [1.0, -1.5, 2.0],
            [1.2, -1.7, 2.2],
            [1.4, -1.9, 2.4],
            [1.6, 2.1, 2.6],
            [1.8, -2.3, 2.8],
            [2.0, 0.0, 3.0],
            [2.2, 2.7, 3.2],
            [2.4, -2.9, 3.4],
            [2.6, 0.1, 3.6],
            [2.8, -3.3, 3.8]
        ];

        let curr_spread = 1.0;
        let result = mid_price_regression(mid_price, features, curr_spread).unwrap();
        println!("Result: {}", result);
    }
}
