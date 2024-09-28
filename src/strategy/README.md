## MARKET MAKER LOGIC

- Creates a new marketmaker with

  - Initial state
  - assets/balance of each acccount or symbol
  - max amount of orders per side
  - final order distance <!-- This will be deleted on this iteration and based on the leverage set by the user --->
  - depths <!-- This is the depths at which to calculate features, this will be adjusted to handle a dynamic amount of depths>
  - leverage
  - rate limit

  # Market Maker Module

  This module implements a market making strategy for cryptocurrency trading. It manages order books, trades, and quote generation across multiple symbols and exchanges.

  ## Key Components

  1. `MarketMaker`: Main struct that encapsulates the market making logic.
  2. `Engine`: Handles feature calculations and market analysis.
  3. `QuoteGenerator`: Generates and manages quotes for each symbol.

  ## Features

  - Multi-symbol support
  - Multi-exchange support (Bybit and Binance)
  - Real-time order book and trade processing
  - Dynamic feature calculation (imbalance, VOI, OFI, etc.)
  - Customizable market making parameters
  - Automatic leverage setting
  - Spread management

  ## Usage

  1. Initialize the `MarketMaker`:

  ```rust
  let market_maker = MarketMaker::new(
      shared_state,
      assets,
      leverage,
      orders_per_side,
      final_order_distance,
      depths,
      rate_limit,
      tick_window
  ).await;
  ```

  2. Start the main loop:

  ```rust
  market_maker.start_loop(receiver).await;
  ```

  3. Set spreads (either manually or from TOML config):

  ```rust
  market_maker.set_spread_bps_input();
  // or
  market_maker.set_spread_toml(bps_vector);
  ```

  ## Configuration

  - `leverage`: Trading leverage
  - `orders_per_side`: Number of orders to place on each side
  - `final_order_distance`: Distance of the final order from mid price
  - `depths`: Depths for calculating imbalance ratios
  - `rate_limit`: API rate limit
  - `tick_window`: Number of ticks for certain calculations

  ## Key Methods

  - `update_features`: Updates market features based on new data
  - `potentially_update`: Updates the strategy with new market and private data
  - `build_features`: Initializes feature engines for each symbol
  - `build_generators`: Sets up quote generators for each symbol

  ## Dependencies

  - `bybit`, `skeleton`: For exchange interactions
  - `tokio`: For asynchronous operations
  - Standard Rust libraries (`std::collections`, `std::time`)

  ## Future Improvements

  - Add support for more exchanges
  - Implement advanced risk management features
  - Optimize performance for high-frequency trading
  - Enhance error handling and logging

  ## Contributing

  Contributions to improve the market making strategy, add new features, or optimize performance are welcome. Please submit pull requests with detailed descriptions of changes.

  Note: This market maker is designed for educational and research purposes. Use in live trading environments at your own risk.
