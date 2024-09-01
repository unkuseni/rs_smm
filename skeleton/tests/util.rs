#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bybit::model::{Ask, Bid, WsTrade};
    use skeleton::util::{
        candles::{TickCandle, VolumeCandle},
        helpers::{
            generate_timestamp, geometric_weights, geomspace, read_toml, round_step, spread_price_in_bps, watch_config, Round
        },
        localorderbook::LocalBook,
    };
    use tokio::sync::mpsc;

    #[test]
    fn test_tick_candle() {
        let mut trades = Vec::new();
        trades.push(WsTrade {
            timestamp: 0,
            symbol: "BTCUSD".to_string(),
            side: "Buy".to_string(),
            tick_direction: "PlusTick".to_string(),
            id: "0".to_string(),
            buyer_is_maker: false,
            price: 100.0,
            volume: 1.0,
        });
        trades.push(WsTrade {
            timestamp: 1,
            symbol: "BTCUSD".to_string(),
            side: "Sell".to_string(),
            tick_direction: "MinusTick".to_string(),
            id: "1".to_string(),
            buyer_is_maker: false,
            price: 101.0,
            volume: 1.0,
        });
        trades.push(WsTrade {
            timestamp: 2,
            symbol: "BTCUSD".to_string(),
            side: "Buy".to_string(),
            tick_direction: "PlusTick".to_string(),
            id: "2".to_string(),
            buyer_is_maker: false,
            price: 102.0,
            volume: 1.0,
        });
        trades.push(WsTrade {
            timestamp: 3,
            symbol: "BTCUSD".to_string(),
            side: "Sell".to_string(),
            tick_direction: "MinusTick".to_string(),
            id: "3".to_string(),
            buyer_is_maker: false,
            price: 103.0,
            volume: 1.0,
        });
        trades.push(WsTrade {
            timestamp: 4,
            symbol: "BTCUSD".to_string(),
            side: "Buy".to_string(),
            tick_direction: "PlusTick".to_string(),
            id: "4".to_string(),
            buyer_is_maker: false,
            price: 104.0,
            volume: 1.0,
        });

        let candles = TickCandle::new(trades, 3);
        assert_eq!(candles.len(), 2);
        let candle = &candles[0];
        assert_eq!(candle.open, 100.0);
        assert_eq!(candle.close, 102.0);
        assert_ne!(candle.high, 104.0);
        assert_eq!(candle.low, 100.0);
        assert_eq!(candles[1].volume, 2.0);
    }

    #[test]
    fn test_volume_candle() {
        let mut trades = Vec::new();
        trades.push(WsTrade {
            timestamp: 0,
            symbol: "BTCUSD".to_string(),
            side: "Buy".to_string(),
            tick_direction: "PlusTick".to_string(),
            id: "0".to_string(),
            buyer_is_maker: false,
            price: 100.0,
            volume: 1.0,
        });
        trades.push(WsTrade {
            timestamp: 1,
            symbol: "BTCUSD".to_string(),
            side: "Sell".to_string(),
            tick_direction: "MinusTick".to_string(),
            id: "1".to_string(),
            buyer_is_maker: false,
            price: 101.0,
            volume: 1.0,
        });
        trades.push(WsTrade {
            timestamp: 2,
            symbol: "BTCUSD".to_string(),
            side: "Buy".to_string(),
            tick_direction: "PlusTick".to_string(),
            id: "2".to_string(),
            buyer_is_maker: false,
            price: 102.0,
            volume: 2.0,
        });
        trades.push(WsTrade {
            timestamp: 3,
            symbol: "BTCUSD".to_string(),
            side: "Sell".to_string(),
            tick_direction: "MinusTick".to_string(),
            id: "3".to_string(),
            buyer_is_maker: false,
            price: 103.0,
            volume: 2.0,
        });
        trades.push(WsTrade {
            timestamp: 4,
            symbol: "BTCUSD".to_string(),
            side: "Buy".to_string(),
            tick_direction: "PlusTick".to_string(),
            id: "4".to_string(),
            buyer_is_maker: false,
            price: 104.0,
            volume: 3.0,
        });

        let candles = VolumeCandle::new(trades, 3.0);
        assert_eq!(candles.len(), 2);
        let candle = &candles[0];
        assert_eq!(candle.open, 100.0);
        assert_eq!(candle.close, 102.0);
        assert_ne!(candle.high, 104.0);
        assert_eq!(candle.low, 100.0);
        assert_eq!(candles[1].volume_threshold, 3.0);
    }

    #[test]
    fn test_round() {
        assert_eq!(round_step(15643.456, 1.0), 15643.0);
        assert_eq!(round_step(5.6567422344, 0.0005), 5.6565);
        println!("{:#?}", spread_price_in_bps(0.00055, 0.5678));
    }

    #[test]
    fn test_time() {
        assert_ne!(generate_timestamp(), 0);
        println!("{:#?}", generate_timestamp());
        let num: f64 = 0.0000016;
        println!("{:#?}", num.abs().round_to(6));
    }

    #[test]
    fn test_places() {
        let num: f64 = 0.000001;
        println!("{:#?}", num.abs().count_decimal_places());
    }

    #[test]
    fn lin() {
        let num_geom = geomspace(0.6243, 0.6001, 5);
        let num_wei = geometric_weights(0.63, 5, true);
        let rev_geom = geomspace(0.6954, 0.6245, 5);

        let rev_wei = geometric_weights(0.37, 5, false);

        println!("{:#?}    {:#?}", num_geom, num_wei);

        println!("{:#?}    {:#?}", rev_geom, rev_wei);
    }

    #[test]
    fn params() {
        let result = read_toml("./tests/test.toml");
        println!("{:#?}", result);
    }


    #[tokio::test]
    async fn test_watch_file() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let path = "./tests/test.toml";
        tokio::spawn(async move {
            let _ = watch_config(path, Duration::from_millis(500), tx).await;
        });
        while let Some(v) = rx.recv().await {
            println!("Config: {:#?}", v);
        }
    }

    #[test]
    fn test_local_book() {
        let mut book = LocalBook::new();
        book.update_bba(
            vec![Bid {
                price: 100.0,
                qty: 1.0,
            }],
            vec![Ask {
                price: 101.0,
                qty: 1.0,
            }],
            10,
        );
        assert_eq!(book.best_bid.price, 100.0);
        assert_eq!(book.best_ask.price, 101.0);
        assert_eq!(book.mid_price, 100.5);
        assert_eq!(book.get_spread(), 1.0);
        assert_eq!(book.get_spread_in_bps(), 99);
    }
}
