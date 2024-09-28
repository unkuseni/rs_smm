# Market Microstructure Features Module

This module implements a suite of advanced market microstructure features for high-frequency trading and market making applications. It provides tools for analyzing order book dynamics, trade flows, and price movements to generate insights for trading strategies.

## Key Components

1. `Engine`: Core component that integrates multiple features and generates a market skew indicator.

2. `imbalance.rs`: Implements order book imbalance metrics:
   - Imbalance ratio
   - Order Flow Imbalance (OFI)
   - Volume at the Offset (VOI)
   - Trade imbalance

3. `impact.rs`: Calculates various price impact and return metrics:
   - Price impact of trades
   - Expected value and improved expected value
   - Mid-price changes and basis
   - Expected returns
   - Price fluctuations
   - Average trade price

4. `linear_reg.rs`: Provides linear regression tools for price prediction:
   - Mid-price regression using multiple features
   - Single-feature regression

## Key Features

- **Order Book Imbalance**: Measures buying/selling pressure at different depths.
- **Order Flow Analysis**: Tracks changes in order placement and cancellation.
- **Price Impact Estimation**: Assesses how trades affect market prices.
- **Return Calculations**: Computes expected returns and price changes.
- **Trade Analysis**: Analyzes trade flow and imbalances.
- **Price Prediction**: Uses linear regression for short-term price forecasting.

## Usage

The `Engine` struct serves as the main interface for feature calculation:

```rust
let mut engine = Engine::new(tick_window);
engine.update(&curr_book, &prev_book, &curr_trades, &prev_trades, &prev_avg, depth_levels);
let skew = engine.skew;
```

## Configuration

Adjust weights in `engine.rs` to fine-tune the skew calculation:

```rust
const IMB_WEIGHT: f64 = 0.25;
const DEEP_IMB_WEIGHT: f64 = 0.10;
const VOI_WEIGHT: f64 = 0.10;
const OFI_WEIGHT: f64 = 0.20;
const DEEP_OFI_WEIGHT: f64 = 0.10;
const PREDICT_WEIGHT: f64 = 0.25;
```

## Dependencies

- `ndarray`: For numerical computations
- `linfa`: For linear regression models
- `bybit`: For trade data structures

## Future Improvements

- Implement more sophisticated regression models (e.g., ARIMA, GARCH)
- Add support for more exchanges and data sources
- Optimize performance for ultra-low latency environments

## Contributing

Contributions are welcome! Please submit pull requests with new features, improvements, or bug fixes.
