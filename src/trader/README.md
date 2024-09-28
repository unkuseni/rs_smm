# Quote Generator Module

This module implements a sophisticated quote generation system for cryptocurrency market making. It's designed to create and manage orders based on current market conditions, order book state, and trading parameters.

## Key Components

1. `BatchOrder`: Represents an order to be placed or cancelled in a batch operation.
2. `OrderManagement`: Enum representing different exchange clients (Bybit and Binance).
3. `QuoteGenerator`: Main struct responsible for generating and managing quotes.
4. `LiveOrder`: Represents an active order in the market.

## Features

- Dynamic spread adjustment based on market conditions
- Inventory management to avoid over-exposure
- Positive and negative skew order generation strategies
- Batch order placement and cancellation
- Rate limiting to comply with exchange API restrictions
- Order book analysis for optimal quote placement
- Support for multiple exchanges (Bybit and Binance)

## Main Functions

- `generate_quotes`: Creates a set of orders based on current market conditions and skew.
- `positive_skew_orders` and `negative_skew_orders`: Generate orders for different market scenarios.
- `send_batch_orders`: Sends a batch of orders to the exchange.
- `check_for_fills`: Processes filled orders and updates positions.
- `out_of_bounds`: Determines if current orders need updating.
- `update_grid`: Core function for updating the order grid based on new market data.

## Usage

The `QuoteGenerator` is typically used as part of a larger market making system. Here's a basic usage example:

```rust
let quote_gen = QuoteGenerator::new(
    client,
    asset,
    leverage,
    orders_per_side,
    final_order_distance,
    rate_limit
);

// In your main loop:
quote_gen.update_grid(private_data, skew, order_book, symbol).await;
```

## Configuration

Key parameters for the `QuoteGenerator` include:
- `minimum_spread`: Minimum spread to use for quote generation
- `max_position_usd`: Maximum position size in USD
- `final_order_distance`: Distance of the final order from the mid price
- `rate_limit`: API rate limit

## Dependencies

- `binance` and `bybit` crates for exchange API interactions
- `tokio` for asynchronous operations
- Custom `skeleton` crate for shared utilities and exchange abstractions

## Note

This module is designed for use in a live trading environment. Ensure proper testing and risk management before deployment with real funds.

## Future Improvements

- Implement more sophisticated pricing models
- Add support for additional exchanges
- Enhance error handling and logging
- Implement dynamic parameter adjustment based on market conditions
