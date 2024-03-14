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
        let mut volume = 0.0;
        let mut open = 0.0;
        let mut close = 0.0;
        let mut high = f64::MIN;
        let mut low = f64::MAX;

        for trade in trades {
            tick_count += 1;
            volume += trade.volume;

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
                    volume,
                });

                tick_count = 0;
                volume = 0.0;
                open = 0.0; // Reset open price for the next candle
                high = f64::MIN;
                low = f64::MAX;
            }
        }

        // Handle the last partial candle if necessary
        if tick_count > 0 {
            candles.push(TickCandle {
                open,
                high,
                low,
                close,
                volume,
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
        let mut open = 0.0;
        let mut close = 0.0;
        let mut high = f64::MIN;
        let mut low = f64::MAX;

        for trade in trades {
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
                open = 0.0; // Reset open price for the next candle
                high = f64::MIN;
                low = f64::MAX;
            }
        }

        // Handle the last partial candle if necessary
        if current_volume > 0.0 {
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
