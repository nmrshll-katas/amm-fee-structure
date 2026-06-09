
/// Strategy parameters (in basis points).
    #[derive(Debug, Clone)]
    pub struct StrategyParams {
        pub base_fee_bps: u64,
        pub widen_threshold_bps: u64,
        pub decay_rate_bps: u64,
        pub name_suffix: String,
    }

    /// Generate a Solidity source string from the given parameters.
    pub fn generate_strategy_solidity(params: &StrategyParams, strategy_name: &str) -> String {
        format!(
            r#"// SPDX-License-Identifier: MIT
      pragma solidity ^0.8.24;

      import {{AMMStrategyBase}} from "./AMMStrategyBase.sol";
      import {{TradeInfo}} from "./IAMMStrategy.sol";

      /// @title Auto-generated Strategy
      /// @notice Strategy variant: {suffix}
      contract Strategy is AMMStrategyBase {{
          uint256 public constant BASE_FEE = {base_fee} * BPS;
          uint256 public constant WIDEN_AMOUNT = {widen} * BPS;
          uint256 public constant DECAY_AMOUNT = {decay} * BPS;
          uint256 public constant WIDEN_THRESHOLD = WAD / 20; // 5% of reserves

          function afterInitialize(
              uint256,
              uint256
          ) external override returns (uint256, uint256) {{
              slots[0] = BASE_FEE;
              return (BASE_FEE, BASE_FEE);
          }}

          function afterSwap(
              TradeInfo calldata trade
          ) external override returns (uint256, uint256) {{
              uint256 fee = slots[0];

              // Check if trade was large relative to reserves
              uint256 tradeRatio = wdiv(trade.amountY, trade.reserveY);
              if (tradeRatio > WIDEN_THRESHOLD) {{
                  // Large trade: widen the spread
                  fee = clampFee(fee + WIDEN_AMOUNT);
              }} else {{
                  // Decay back toward base fee
                  uint256 base = BASE_FEE;
                  if (fee > base) fee = fee - DECAY_AMOUNT;
              }}

              slots[0] = fee;
              return (fee, fee);
          }}

          function getName() external pure override returns (string memory) {{
              return "{strategy_name}";
          }}
      }}
      "#,
            suffix = params.name_suffix,
            base_fee = params.base_fee_bps,
            widen = params.widen_threshold_bps,
            decay = params.decay_rate_bps,
            strategy_name = strategy_name
        )
    }
