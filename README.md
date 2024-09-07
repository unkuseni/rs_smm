### A SIMPLE MARKETMAKER IN RUST

This is a simple implementation of a market_maker in rust

### Table of Contents

- Shared state
- Market maker
- Feature Engine
- Quote Generator
- Parameters

NOTE: On the change, rewrite for watching the toml file for changes

1. The main function is marked with `#[tokio::main]`, which means it's an asynchronous function that will be run by the Tokio runtime.

2. It starts by reading a configuration file:

   ```rust
   let config = use_toml();
   ```

   This function likely reads a TOML configuration file and returns a struct with the parsed configuration.

3. It initializes a shared state:

   ```rust
   let mut state = ss::SharedState::new(config.exchange);
   ```

   This creates a new `SharedState` object with the exchange specified in the config.

4. It adds symbols to the state:

   ```rust
   state.add_symbols(config.symbols);
   ```

   This adds the symbols specified in the config to the shared state.

5. It adds clients for each API key:

   ```rust
   let clients = config.api_keys;
   for (key, secret, symbol) in clients {
       state.add_clients(key, secret, symbol, None);
   }
   ```

   This loop adds a client for each set of API keys specified in the config.

6. It creates a balance hashmap:

   ```rust
   let balance = balances(config.balances);
   ```

   This function likely converts the balance data from the config into a HashMap.

7. It initializes the market maker:

   ```rust
   let mut market_maker = MarketMaker::new(
       state.clone(),
       balance,
       config.leverage,
       config.orders_per_side,
       config.final_order_distance,
       config.depths,
       config.rate_limit,
       config.tick_window,
   );
   ```

   This creates a new `MarketMaker` instance with various parameters from the config.

8. It sets the spread for the market maker:

   ```rust
   market_maker.set_spread_toml(config.bps);
   ```

   This sets the spread based on the basis points specified in the config.

9. It creates an unbounded channel:

   ```rust
   let (sender, receiver) = mpsc::unbounded_channel();
   ```

   This channel will be used to send updates from the shared state to the market maker.

10. It spawns a new task to load data:

    ```rust
    tokio::spawn(async move {
        ss::load_data(state, sender).await;
    });
    ```

    This starts a new asynchronous task that loads data into the shared state and sends updates through the channel.

11. Finally, it starts the market maker loop:
    ```rust
    market_maker.start_loop(receiver).await;
    ```
    This starts the main loop of the market maker, which will receive updates from the shared state and make trading decisions.

In summary, this main function sets up the entire trading system: it reads the configuration, initializes the state and market maker, sets up communication channels, and starts the data loading and trading processes. The actual trading logic would be implemented in the `MarketMaker` struct's methods.
