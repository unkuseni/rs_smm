use bybit::model::WsTrade;
pub struct TickCandle {
    pub open: f64,
    pub close: f64,
    pub high: f64,
    pub low: f64,
    pub volume: f64,
}

impl TickCandle {
    pub fn new(trades: Vec<WsTrade>, ticks: usize) -> Vec<TickCandle> {
        let mut candles: Vec<TickCandle> = Vec::new();
        let mut tick_count = 0;
        let mut bucket_trades: Vec<WsTrade> = Vec::new();
        let mut open = 0.0;
        let mut close = 0.0;
        let mut high = f64::MIN;
        let mut low = f64::MAX;

        for trade in trades {
            bucket_trades.push(trade.clone());
            tick_count += 1;

            open = if open == 0.0 { trade.price } else { open };
            close = trade.price; // Update the close price for each trade
            high = f64::max(high, trade.price);
            low = f64::min(low, trade.price);

            if tick_count >= ticks {
                candles.push(TickCandle {
                    open,
                    high,
                    low,
                    close,
                    volume: bucket_trades.iter().map(|t| t.volume).sum(),
                });

                tick_count = 0;
                bucket_trades.clear();
                open = 0.0; // Reset open price for the next candle
                high = f64::MIN;
                low = f64::MAX;
            }
        }

        // Handle the last partial candle if necessary
        if !bucket_trades.is_empty() {
            candles.push(TickCandle {
                open,
                high,
                low,
                close,
                volume: bucket_trades.iter().map(|t| t.volume).sum(),
            });
        }

        candles
    }
}
pub struct VolumeCandle {
    pub open: f64,
    pub close: f64,
    pub high: f64,
    pub low: f64,
    pub volume_threshold: f64,
}

impl VolumeCandle {
    pub fn new(trades: Vec<WsTrade>, volume_threshold: f64) -> Vec<VolumeCandle> {
        let mut candles: Vec<VolumeCandle> = Vec::new();
        let mut current_volume = 0.0;
        let mut candle_trades: Vec<WsTrade> = Vec::new();
        let mut open = 0.0;
        let mut close = 0.0;
        let mut high = f64::MIN;
        let mut low = f64::MAX;

        for trade in trades {
            candle_trades.push(trade.clone());
            current_volume += trade.volume;

            open = if open == 0.0 { trade.price } else { open };
            close = trade.price; // Update the close price for each trade
            high = f64::max(high, trade.price);
            low = f64::min(low, trade.price);

            if current_volume >= volume_threshold {
                candles.push(VolumeCandle {
                    open,
                    close,
                    high,
                    low,
                    volume_threshold,
                });

                current_volume = 0.0;
                candle_trades.clear();
                open = 0.0; // Reset open price for the next candle
            }
        }

        // Handle the last partial candle if necessary
        if !candle_trades.is_empty() {
            candles.push(VolumeCandle {
                open,
                close,
                high,
                low,
                volume_threshold: current_volume, // Note: this is less than the threshold
            });
        }

        candles
    }
}
