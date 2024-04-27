 use bybit::model::WsTrade;
 
 // This module contains two structs: TickCandle and VolumeCandle.
// These structs are used to create candlestick charts based on tick or volume thresholds.

// The TickCandle struct represents a single candlestick chart based on a number of ticks.
// It contains the following fields:
// - open: the price at the start of the candle
// - close: the price at the end of the candle
// - high: the highest price in the candle
// - low: the lowest price in the candle
// - volume: the total volume traded in the candle
pub struct TickCandle {
    pub open: f64,
    pub close: f64,
    pub high: f64,
    pub low: f64,
    pub volume: f64,
}

// The TickCandle struct has an associated function called 'new'.
// This function takes in two arguments: a vector of trades and a number of ticks.
// It creates a new vector of TickCandle structs based on the input data.
impl TickCandle {
    // This function iterates over each trade in the input vector and calculates the candlestick chart.
    // It uses the following variables:
    // - candles: a vector to store the resulting candlestick charts
    // - tick_count: the number of trades in the current candle
    // - volume: the total volume traded in the current candle
    // - open: the price at the start of the candle
    // - close: the price at the end of the candle
    // - high: the highest price in the candle
    // - low: the lowest price in the candle
    // It iterates over each trade and performs the following actions:
    // - increments the tick_count
    // - adds the trade volume to the volume
    // - updates the open and close prices based on the trade price
    // - updates the high and low prices based on the trade price
    // - if the tick_count is greater than or equal to the ticks argument, it creates a new TickCandle struct and adds it to the candles vector
    // - resets the tick_count, volume, open, high, and low variables for the next candle
    // - at the end, if there is a partial candle, it creates a new TickCandle struct and adds it to the candles vector
    // It returns the candles vector.
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

// The VolumeCandle struct represents a single candlestick chart based on a volume threshold.
// It contains the following fields:
// - open: the price at the start of the candle
// - close: the price at the end of the candle
// - high: the highest price in the candle
// - low: the lowest price in the candle
// - volume_threshold: the volume threshold for the candle
pub struct VolumeCandle {
    pub open: f64,
    pub close: f64,
    pub high: f64,
    pub low: f64,
    pub volume_threshold: f64,
}

// The VolumeCandle struct has an associated function called 'new'.
// This function takes in two arguments: a vector of trades and a volume threshold.
// It creates a new vector of VolumeCandle structs based on the input data.
impl VolumeCandle {
    // This function iterates over each trade in the input vector and calculates the candlestick chart.
    // It uses the following variables:
    // - candles: a vector to store the resulting candlestick charts
    // - current_volume: the total volume traded in the current candle
    // - open: the price at the start of the candle
    // - close: the price at the end of the candle
    // - high: the highest price in the candle
    // - low: the lowest price in the candle
    // It iterates over each trade and performs the following actions:
    // - adds the trade volume to the current_volume
    // - updates the open and close prices based on the trade price
    // - updates the high and low prices based on the trade price
    // - if the current_volume is greater than or equal to the volume_threshold, it creates a new VolumeCandle struct and adds it to the candles vector
    // - resets the current_volume, open, close, high, and low variables for the next candle
    // - at the end, if there is a partial candle, it creates a new VolumeCandle struct and adds it to the candles vector
    // It returns the candles vector.
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

