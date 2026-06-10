// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {AMMStrategyBase} from "../AMMStrategyBase.sol";
import {IAMMStrategy, TradeInfo} from "../IAMMStrategy.sol";

contract Strategy is AMMStrategyBase {
    // Constants
    uint256 private constant BASE_FEE_BPS = 30; // range(0-100 by 1)
    uint256 private constant LARGE_TRADE_THRESHOLD_DIVISOR = 20; // range(2-100 by 1)
    uint256 private constant FEE_INCREMENT_BPS = 10; // range(0-50 by 1) - Maximum 0.5% increase per large trade
    uint256 private constant FEE_DECAY_BPS = 1; // range(0-20 by 1) - Maximum 0.2% decay per small trade

    function afterInitialize(
        uint256,
        uint256
    ) external override returns (uint256, uint256) {
        slots[0] = bpsToWad(BASE_FEE_BPS);
        return (bpsToWad(BASE_FEE_BPS), bpsToWad(BASE_FEE_BPS));
    }

    function afterSwap(
        TradeInfo calldata trade
    ) external override returns (uint256, uint256) {
        uint256 fee = slots[0];

        // Large trade relative to reserves? Widen the spread.
        uint256 tradeRatio = wdiv(trade.amountY, trade.reserveY);
        if (tradeRatio > WAD / LARGE_TRADE_THRESHOLD_DIVISOR) {
            // > 5% of reserves
            fee = clampFee(fee + bpsToWad(FEE_INCREMENT_BPS));
        } else {
            // Decay back toward base fee (30 bps)
            uint256 base = bpsToWad(BASE_FEE_BPS);
            if (fee > base) fee = fee - bpsToWad(FEE_DECAY_BPS);
        }

        slots[0] = fee;
        return (fee, fee);
    }

    function getName() external pure override returns (string memory) {
        return "WidenAfterBigTrades";
    }
}
