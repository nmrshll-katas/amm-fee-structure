// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {AMMStrategyBase} from "../AMMStrategyBase.sol";
import {IAMMStrategy, TradeInfo} from "../IAMMStrategy.sol";

/// @title Volatility Tracking Fee Strategy
/// @notice Estimates realized volatility from past price changes and adjusts fee linearly
/// @dev Uses EWMA of squared returns. Fee = base + sensitivity * sigma
contract Strategy is AMMStrategyBase {
    /// @notice Smoothing factor for EWMA (0.9 = 90%). Must be in WAD.
    /// @dev Higher lambda = more weight on recent values
    uint256 public constant LAMBDA_WAD = 9e17; // range(1e17-1e18 by 1e17)

    /// @notice Base fee applied to all transactions (14 bps = 0.0014%)
    /// @dev In WAD units for fee calculations
    uint256 public constant BASE_FEE_WAD = 14 * BPS; // range(1e12-4e12 by 1e12)

    /// @notice Sensitivity multiplier for volatility/sigma (2.0x). Must be in WAD.
    /// @dev Scales raw volatility to fee adjustment magnitude
    uint256 public constant SENSITIVITY_WAD = 2e18; // range(1e18-5e18 by 1e17)

    // --- Storage slots (0..2) ---
    // slot0: last observed price (reserveY * WAD / reserveX)
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
        slots[2] = 1e12; // Initial variance guess: 1e-6 true variance -> 1e12 WAD

        // Return initial fee (base fee only, no volatility history)
        uint256 fee = BASE_FEE_WAD;
        return (fee, fee);
    }

    /// @inheritdoc IAMMStrategy
    function afterSwap(
        TradeInfo calldata trade
    ) external override returns (uint256 bidFee, uint256 askFee) {
        uint256 newPrice = (trade.reserveY * WAD) / trade.reserveX;

        uint256 lastPrice = slots[0];
        uint256 lastTs = slots[1];
        uint256 variance = slots[2];

        // Time difference (should be >= 1, but guard anyway)
        uint256 dt = trade.timestamp - lastTs;
        if (dt > 0 && lastPrice > 0) {
            // Approximate log return: (ΔP / P) in WAD
            uint256 priceDiff = newPrice > lastPrice
                ? newPrice - lastPrice
                : lastPrice - newPrice;
            uint256 returnWad = (priceDiff * WAD) / lastPrice;
            // Sign does not matter for variance, we use absolute diff.

            uint256 returnSqWad = wmul(returnWad, returnWad);

            // Observed variance increment
            uint256 obsWad = returnSqWad / dt;

            // EWMA update: variance = lambda * variance + (1-lambda) * obs
            variance =
                (LAMBDA_WAD * variance + (WAD - LAMBDA_WAD) * obsWad) /
                WAD;
        }

        // Compute sigma = sqrt(variance * WAD)
        uint256 sigmaWad = sqrt(variance * WAD);

        // Fee = base + sensitivity * sigma
        uint256 feeWad = BASE_FEE_WAD + (SENSITIVITY_WAD * sigmaWad) / WAD;
        feeWad = clampFee(feeWad); // between 0 and 10%

        // Store updated state
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
