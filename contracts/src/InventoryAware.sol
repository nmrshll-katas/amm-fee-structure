// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {AMMStrategyBase} from "./AMMStrategyBase.sol";
import {IAMMStrategy, TradeInfo} from "./IAMMStrategy.sol";

/// @title Inventory‑Aware Fee Strategy
/// @notice Adjusts fee based on squared deviation from neutral price
/// @dev fee = baseFee * (1 + k * deviation^2)
contract Strategy is AMMStrategyBase {
    // --- Constants ---
    uint256 public constant K_WAD = 5e18; // k = 5 (amplification factor)
    uint256 public constant BASE_FEE_WAD = 20 * BPS; // 20 bps base fee

    // --- Storage slots ---
    // slots[0] = neutralPrice (mid price at initialization, WAD)
    // slots[1] = (unused, reserved for future adjustments)

    /// @inheritdoc IAMMStrategy
    function afterInitialize(
        uint256 initialX,
        uint256 initialY
    ) external override returns (uint256 bidFee, uint256 askFee) {
        // Compute neutral price = reserveY / reserveX (WAD)
        uint256 neutralPrice = (initialY * WAD) / initialX;
        slots[0] = neutralPrice; // store neutral price permanently

        // Return base fee (no deviation yet)
        return (BASE_FEE_WAD, BASE_FEE_WAD);
    }

    /// @inheritdoc IAMMStrategy
    function afterSwap(
        TradeInfo calldata trade
    ) external view override returns (uint256 bidFee, uint256 askFee) {
        // 1. Current price from post‑trade reserves
        uint256 currentPrice = (trade.reserveY * WAD) / trade.reserveX;

        // 2. Load neutral price (set once at initialization)
        uint256 neutralPrice = slots[0];
        if (neutralPrice == 0) {
            // Safety fallback – should never happen
            return (BASE_FEE_WAD, BASE_FEE_WAD);
        }

        // 3. Compute absolute deviation: |current - neutral| / neutral
        uint256 diff;
        if (currentPrice >= neutralPrice) {
            diff = currentPrice - neutralPrice;
        } else {
            diff = neutralPrice - currentPrice;
        }
        uint256 deviationWad = (diff * WAD) / neutralPrice; // deviation in WAD

        // 4. Squared deviation (WAD)
        uint256 deviationSqWad = wmul(deviationWad, deviationWad);

        // 5. Factor = 1 + k * deviation^2  (all in WAD)
        uint256 factor = WAD + wmul(K_WAD, deviationSqWad);

        // 6. fee = baseFee * factor
        uint256 feeWad = wmul(BASE_FEE_WAD, factor);
        feeWad = clampFee(feeWad); // between 0 and 10%

        return (feeWad, feeWad);
    }

    /// @inheritdoc IAMMStrategy
    function getName() external pure override returns (string memory) {
        return "InventoryAware";
    }
}
