// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {AMMStrategyBase} from "../AMMStrategyBase.sol";
import {IAMMStrategy, TradeInfo} from "../IAMMStrategy.sol";

/// @title Asymmetric Price Deviation Fee Strategy
/// @notice Raises the fee on trades that move the pool further from its starting price.
contract Strategy is AMMStrategyBase {
    uint256 internal constant INITIAL_FEE_WAD = 12 * BPS; // range(1e14-1e16 by 1e14)
    uint256 internal constant BASE_FEE_WAD = 25 * BPS; // range(1e14-1e16 by 1e14)
    uint256 internal constant K_FACTOR = 250 * BPS; // range(0-1e17 by 1e15)

    /// @inheritdoc IAMMStrategy
    function afterInitialize(
        uint256 initialX,
        uint256 initialY
    ) external override returns (uint256 bidFee, uint256 askFee) {
        slots[0] = initialX == 0 ? 0 : wdiv(initialY, initialX);
        return (INITIAL_FEE_WAD, INITIAL_FEE_WAD);
    }

    /// @inheritdoc IAMMStrategy
    function afterSwap(
        TradeInfo calldata trade
    ) external view override returns (uint256 bidFee, uint256 askFee) {
        uint256 neutralPrice = slots[0];
        if (neutralPrice == 0 || trade.reserveX == 0) {
            return (BASE_FEE_WAD, BASE_FEE_WAD);
        }

        uint256 price = wdiv(trade.reserveY, trade.reserveX);
        uint256 deviation = wdiv(absDiff(price, neutralPrice), neutralPrice);
        uint256 adjustment = wmul(K_FACTOR, deviation);
        uint256 higherFee = clampFee(BASE_FEE_WAD + adjustment);
        uint256 discount = adjustment / 2;
        uint256 lowerFee = discount < BASE_FEE_WAD ? BASE_FEE_WAD - discount : MIN_FEE;

        return price >= neutralPrice ? (lowerFee, higherFee) : (higherFee, lowerFee);
    }

    /// @inheritdoc IAMMStrategy
    function getName() external pure override returns (string memory) {
        return "AsymmetricPriceDeviation";
    }
}
