# Market Maker Parameters Module

This module provides functionality for configuring and managing parameters for a market making system. It includes functions for user input, parameter storage, and configuration management.

## Key Components

1. `watch`: A utility function for prompting user input.

2. Parameter Collection Functions:
   - `exch_params`: Collects exchange selection.
   - `symbol_params`: Collects trading symbols.
   - `api_key_params`: Collects API keys for different symbols.
   - `acct_balance_params`: Collects account balances for different symbols.
   - `maker_params`: Collects market maker specific parameters.

3. `use_toml`: Reads configuration from a TOML file.

4. `MakerParams`: A struct to store market maker parameters.

## Usage

### User Input

Use the parameter collection functions to gather user input:

```rust
let exchange = exch_params();
let symbols = symbol_params();
let api_keys = api_key_params();
let balances = acct_balance_params();
let maker_params = maker_params();
```

### TOML Configuration

To use TOML configuration:

```rust
let config = use_toml();
```

### Market Maker Parameters

Create a `MakerParams` instance:

```rust
let params = MakerParams::new(
    leverage,
    orders_per_side,
    final_order_distance,
    depths,
    rebalance_ratio,
    rate_limit
);
```

## Configuration Options

- Exchange selection: "bybit", "binance", or "both"
- Trading symbols
- API keys and secret keys for each symbol
- Account balances for each symbol
- Leverage
- Orders per side
- Final order distance
- Depth levels
- Rebalance ratio
- Rate limit

## Note

The "both" and "binance" options for exchange selection are marked as unstable.

## Dependencies

- `std::collections::HashMap`
- `std::io`
- `skeleton::util::helpers::{read_toml, Config}`

## Future Improvements

- Implement automatic account leverage setting
- Enhance error handling and input validation
- Add support for additional configuration options
- Implement a more robust configuration file format

## Contributing

Contributions for improving the parameter management, adding new configuration options, or enhancing the user interface are welcome.
