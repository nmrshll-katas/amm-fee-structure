### How an AMM’s price lags in volatile markets
- An AMM (using x * y = k) always quotes a price = reserveY / reserveX.
- The true market price moves via GBM. After a price move, the AMM’s price is stale until a trade updates it.
- The larger the volatility (σ), the bigger the expected gap between the AMM’s price and the true price at any moment.

### Arbitrage extraction
Price is kept close to the real price through arbitrage trades.

Each arbitrage trade:
- Moves the AMM’s price back toward the true price.
- Extracts value from liquidity providers (LPs). The AMM sells below true price or buys above true price –> negative edge
Higher volatility → bigger average gaps → larger arbitrage profits per trade → more total value extracted from LPs.

### AMM fees counteract arbitrage extracton
A fee is a spread: the AMM buys at (mid price) * (1 – fee) and sells at (mid price) * (1 + fee) (depending on direction).
- Fees act as a buffer. An arbitrageur only profits if the mispricing exceeds the fee.
- If the expected gap = 0.5% and we set fee = 0.6%, arbitrage is unprofitable. If fee = 0.3%, arbitrageurs still profit
⇒ higher volatility requires a higher fee to deter arbitrage extraction
BUT fees deter retail / legitimate traders ⇒ they go to other exchanges

 ⇒ Goal of dynamic fee structure:  estimate volatility and adjust fees accordingly

### Possible strategies
1. Static fee (example included) → 30 bps regardless of past trades
2. Widen after large trades (example included):
- increase fee after large trade
- slowly decay back to baseline fee
Pros: 
- large trade means either volatility or sophisticated/informed trader
- recovers edge from arbitrage after large retail trade imbalance
Cons: overreact after large retail trade ? → lose volume to 30 bps competitor
3. Estimate volatility (from past trades)
Idea:
- AMM price quotes are like options (guaranteed "stale" price while real price moves)
- "fair" (optimal?) option price (premium) is proportional to σ² (black-scholes)
- optimal_fee ∝ σ² × (current_price / neutral_price – 1)²

4. Similar: depends on volatility but simplified (proportional to variance (σ²):
Use weighted average variance estimation, with smoothing/decay factor (newer values have more weight than older values)
5. Imbalanced fees based on inventory variation: generate different fees for buy/sell, based on the relative change on each asset quantity (inventory-aware). 
Approach: Adjust fees to encourage rebalancing. If the AMM holds too much X, increase fee on buying X to discourage further imbalance, or lower fee on the opposite side to attract balancing flow.
Cons: without an oracle we don't know if the AMM holds "too much X" or if the "real" price (from the outside world) just changed
6. Learning from past edge
Idea: track if recent trades generated positive or negative edge. Increase fee after a sequence of negative‑edge trades (arbitrage), decrease after positive‑edge trades (retail).
Cons: we would need an estimate of the "real" price.


### Design tradeoffs
- no external data access: oracles would be nice to get info about the "real" price and adjust fee accordingly. To see the market price with the same info arbitrageurs have. Instead we must guess from past trades.
- Limited storage: stored data must fit within allocated space block (32 slots)
- Not taken in account: complexity of strategy → more computation → more gas spend for users → flee to competitor exchange

### Switching methodology → optimizing parameters
Manual strategy didn't give great results.
Changing parameters slightly could give relatively big differences
I added an optimizer that goes in 2 phases:
1. exploration: tries random parameters to sample combinations (strategy, params, edge) randomly across the parameter space
2. refinement: takes the previous best combination (strategy, params), and tries varying parameters slightly to see if they can be improved iteratively (hill-climbing)
The optimizing code (rust) lives in "./optimizer"

The optimizer starts with the candidates in `optimizer/seeds.json`. It re-runs
each candidate using the current `--simulations` setting and gives it 1,000
refinement attempts before continuing with the local results database and
random exploration. The JSON keeps the original database IDs and edge scores.

To refresh the seed file from the local database, run this from the repository
root. The exporter keeps the 20 highest-scoring results and adds the two best
distinct parameter sets for each strategy type absent from those 20. It breaks
ties by lowest database ID.

```sh
python3 optimizer/export_seeds.py .cache/db.sqlite optimizer/seeds.json
```

To regenerate the Solidity files from the committed seeds without running
simulations, run this from the repository root.

```sh
cargo run -p optimizer -- --export-seed-solidity .cache/seed_strategies
```

### Links
Optimal Fees for Liquidity Provision in Automated Market Makers:https://ar5iv.labs.arxiv.org/html/2508.08152
