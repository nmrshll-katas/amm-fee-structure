// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {AMMStrategyBase} from "./AMMStrategyBase.sol";
import {IAMMStrategy, TradeInfo} from "./IAMMStrategy.sol";

/// @title Volatility Tracking Fee Strategy
/// @notice Estimates realized volatility from past price changes and adjusts fee linearly
/// @dev Uses EWMA of squared returns. Fee = base + sensitivity * sigma
contract Strategy is AMMStrategyBase {
    // --- Constants ---
    uint256 public constant LAMBDA_WAD = 9e17; // 0.9 smoothing factor
    uint256 public constant BASE_FEE_WAD = 14 * BPS; // 10 bps
    uint256 public constant SENSITIVITY_WAD = 2e18; // multiplier for sigma

    // --- Storage slots (0..2) ---
    // slot0: last observed mid price (reserveY * WAD / reserveX)
    // slot1: last timestamp (step number)
    // slot2: current variance estimate (WAD, true variance * 1e18)

    /// @inheritdoc IAMMStrategy
    function afterInitialize(
        uint256 initialX,
        uint256 initialY
    ) external override returns (uint256 bidFee, uint256 askFee) {
        // Compute initial mid price
        uint256 initialPrice = (initialY * WAD) / initialX;
        slots[0] = initialPrice; // lastPriceWad
        slots[1] = 0; // lastTimestamp
        // Initial variance guess: 1e-6 true variance -> 1e12 WAD
        slots[2] = 1e12;

        // Return initial fee (base fee only, no volatility history)
        uint256 fee = BASE_FEE_WAD;
        return (fee, fee);
    }

    /// @inheritdoc IAMMStrategy
    function afterSwap(
        TradeInfo calldata trade
    ) external override returns (uint256 bidFee, uint256 askFee) {
        // 1. Current price from post-trade reserves
        uint256 newPrice = (trade.reserveY * WAD) / trade.reserveX;

        // 2. Load previous state
        uint256 lastPrice = slots[0];
        uint256 lastTs = slots[1];
        uint256 variance = slots[2];

        // 3. Time difference (should be >= 1, but guard anyway)
        uint256 dt = trade.timestamp - lastTs;
        if (dt > 0 && lastPrice > 0) {
            // 4. Approximate log return: (ΔP / P) in WAD
            uint256 priceDiff = newPrice > lastPrice
                ? newPrice - lastPrice
                : lastPrice - newPrice;
            uint256 returnWad = (priceDiff * WAD) / lastPrice;
            // Sign does not matter for variance, we use absolute diff.

            // 5. Squared return in WAD
            uint256 returnSqWad = wmul(returnWad, returnWad);

            // 6. Observed variance increment for this dt
            uint256 obsWad = returnSqWad / dt;

            // 7. EWMA update: variance = lambda * variance + (1-lambda) * obs
            variance =
                (LAMBDA_WAD * variance + (WAD - LAMBDA_WAD) * obsWad) /
                WAD;
        }

        // 8. Compute sigma = sqrt(variance * WAD)
        uint256 sigmaWad = sqrt(variance * WAD);

        // 9. Fee = base + sensitivity * sigma
        uint256 feeWad = BASE_FEE_WAD + (SENSITIVITY_WAD * sigmaWad) / WAD;
        feeWad = clampFee(feeWad); // between 0 and 10%

        // 10. Store updated state
        slots[0] = newPrice;
        slots[1] = trade.timestamp;
        slots[2] = variance;

        return (feeWad, feeWad);
    }

    /// @inheritdoc IAMMStrategy
    function getName() external pure override returns (string memory) {
        return "VolatilityTracking_EWMA";
    }
}
