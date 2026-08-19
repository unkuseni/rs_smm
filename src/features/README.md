# Market Microstructure Features Module

This module implements a suite of advanced market microstructure features for high-frequency trading and market making applications. It provides tools for analyzing order book dynamics, trade flows, and price movements to generate insights for trading strategies.

## Key Components

1. `Engine`: Core component that integrates multiple features and generates a market skew indicator.

2. `imbalance.rs`: Implements order book imbalance metrics:
   - Imbalance ratio
   - Order Flow Imbalance (OFI)
   - Volume at the Offset (VOI)
   - Trade imbalance

3. `impact.rs`: Calculates various return metrics:
   - Expected value
   - Mid-price basis and expected returns
   - Price fluctuations
   - Average trade price (incremental VWAP)

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

Skew weights, deadbands, gates and the prediction setup are driven by the
`[strategy]` table in `config.toml` (defaults live in
`skeleton::util::helpers::StrategyConfig`):

```toml
[strategy]
imb_weight = 0.25          # top-of-book imbalance
deep_imb_weight = 0.10     # average deep imbalance
trade_weight = 0.30        # trade imbalance
voi_weight = 0.10          # volume of interest (sign only)
deep_ofi_weight = 0.10     # depth-normalized OFI magnitude
predict_weight = 0.15      # regression-based predicted move
```

Notes:
- Deep OFI now enters the skew with its **magnitude**, normalized by the
depth-weighted volume at the widest requested depth (Cont, Kukanov &
Stoikov 2011: price impact is linear in OFI, scaled by inverse depth).
- The regression uses lagged feature pairs `[f_t, f_{t-1}]` (8 columns) to
predict the next mid price (Shen 2015 finds instantaneous and lag-1
imbalance are the significant predictors).
- `Engine::mid_return_vol` exposes the rolling std of mid returns, which the
  quote generator uses for its volatility-adaptive spread (rescaled to
  per-second volatility under the ~10ms update cadence).

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
