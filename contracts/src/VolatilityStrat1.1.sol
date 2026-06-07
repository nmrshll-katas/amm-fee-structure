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
    uint256 public constant BASE_FEE_WAD = 12 * BPS; // 10 bps
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
        uint256 price = (trade.reserveY * WAD) / trade.reserveX;
        uint256 neutral = slots[0];
        uint256 deviation = price > neutral ? price - neutral : neutral - price;
        deviation = (deviation * WAD) / neutral;

        // Base fee 25bps
        uint256 base = 25 * BPS;

        // Asymmetric adjustment based on direction of imbalance
        if (price > neutral) {
            // Price too high (pool has too much Y, too little X)
            // Raise fee on buys (traders buying X from us)
            // Lower fee on sells (traders selling X to us)
            askFee = clampFee(base + (K_FACTOR * deviation) / WAD);
            bidFee = clampFee(base - ((K_FACTOR * deviation) / 2) / WAD);
        } else if (price < neutral) {
            // Price too low (pool has too much X, too little Y)
            // Raise fee on sells (traders selling X to us)
            // Lower fee on buys (traders buying X from us)
            askFee = clampFee(base - ((K_FACTOR * deviation) / 2) / WAD);
            bidFee = clampFee(base + (K_FACTOR * deviation) / WAD);
        } else {
            askFee = base;
            bidFee = base;
        }

        return (bidFee, askFee);
    }

    /// @inheritdoc IAMMStrategy
    function getName() external pure override returns (string memory) {
        return "VolatilityTracking_EWMA";
    }
}
