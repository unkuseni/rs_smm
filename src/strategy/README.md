## MARKET MAKER LOGIC

- Creates a new marketmaker with
    - Initial state
    - assets/balance of each acccount or symbol
    - max amount of orders per side
    - final order distance  <!-- This will be deleted on this iteration and based on the leverage set by the user --->
    - depths <!-- This is the depths at which to calculate features, this will be adjusted to handle a dynamic amount of depths>
    - leverage 
    - rate limit

