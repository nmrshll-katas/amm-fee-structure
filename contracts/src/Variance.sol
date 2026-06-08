// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {AMMStrategyBase} from "./AMMStrategyBase.sol";
import {IAMMStrategy, TradeInfo} from "./IAMMStrategy.sol";

/// @title Vanilla AMM Strategy
/// @notice Default strategy with fixed 30 basis point fees
/// @dev This runs as the second AMM in simulations to normalize scoring
contract Strategy is AMMStrategyBase {
    /// @notice Smoothing factor for EWMA (10% = 1e17). Must be in WAD.
    uint256 internal constant ALPHA = WAD / 10; // 0.1 range(1e16-1e17 by 1e16)

    /// @notice Scaling factor from volatility (WAD) to fee increment (WAD)
    /// @dev e.g., 10% volatility adds 1% fee: (0.1 * 0.1) = 0.01 = 1e16
    uint256 internal constant FEE_SCALE = 1e16; // 1% in WAD

    /// @notice Base fee applied when volatility is zero (5 bps = 5e14)
    uint256 internal constant BASE_FEE = 5 * BPS;

    /// @inheritdoc IAMMStrategy
    function afterInitialize(
        uint256 initialX,
        uint256 initialY
    ) external returns (uint256 bidFee, uint256 askFee) {
        uint256 initialPrice = _getPrice(initialX, initialY);

        // Store last price
        slots[0] = initialPrice;
        // Initialize variance to zero
        slots[1] = 0;

        // Return base fee initially
        bidFee = BASE_FEE;
        askFee = BASE_FEE;
    }

    /// @inheritdoc IAMMStrategy
    function afterSwap(
        TradeInfo calldata trade
    ) external returns (uint256 bidFee, uint256 askFee) {
        uint256 currentPrice = _getPrice(trade.reserveX, trade.reserveY);
        uint256 lastPrice = slots[0];

        // Compute absolute return and squared return
        uint256 returnWAD = _computeReturn(currentPrice, lastPrice);
        uint256 squaredReturn = (returnWAD * returnWAD) / WAD;

        // Update variance estimate
        uint256 varianceOld = slots[1];
        uint256 varianceNew = _updateVariance(varianceOld, squaredReturn);
        slots[1] = varianceNew;

        // Compute volatility = sqrt(variance * WAD) to convert back to WAD percentage
        uint256 volatilityWAD = _sqrt(varianceNew * WAD);

        // Determine fees (symmetric for both sides)
        uint256 fee = _computeFee(volatilityWAD);

        // Update stored price for next trade
        slots[0] = currentPrice;

        return (fee, fee);
    }

    /// @inheritdoc IAMMStrategy
    function getName() external pure returns (string memory) {
        return "ProportionalToVariance";
    }

    /// @notice Computes price from reserves: price = reserveY * WAD / reserveX
    /// @param reserveX X reserve (WAD)
    /// @param reserveY Y reserve (WAD)
    /// @return price in WAD
    function _getPrice(
        uint256 reserveX,
        uint256 reserveY
    ) internal pure returns (uint256) {
        require(reserveX > 0, "VolatilityEstimator: zero reserveX");
        return (reserveY * WAD) / reserveX;
    }

    /// @notice Babylonian square root for a uint256, returns floor(sqrt(x))
    function _sqrt(uint256 x) internal pure returns (uint256) {
        if (x == 0) return 0;
        uint256 z = (x + 1) / 2;
        uint256 y = x;
        while (z < y) {
            y = z;
            z = (x / z + z) / 2;
        }
        return y;
    }

    /// @notice Compute absolute return between two prices as a fraction in WAD
    /// @param priceNew New price (WAD)
    /// @param priceOld Old price (WAD)
    /// @return returnWAD |(priceNew - priceOld)| * WAD / priceOld
    function _computeReturn(
        uint256 priceNew,
        uint256 priceOld
    ) internal pure returns (uint256) {
        if (priceNew > priceOld) {
            return ((priceNew - priceOld) * WAD) / priceOld;
        } else {
            return ((priceOld - priceNew) * WAD) / priceOld;
        }
    }

    /// @notice Update EWMA variance estimate with the latest squared return
    /// @param varianceOld Previous variance (WAD squared)
    /// @param squaredReturn Latest squared return (WAD squared)
    /// @return varianceNew Updated variance
    function _updateVariance(
        uint256 varianceOld,
        uint256 squaredReturn
    ) internal pure returns (uint256) {
        // newVar = (ALPHA * sqReturn + (WAD - ALPHA) * oldVar) / WAD
        uint256 term1 = (ALPHA * squaredReturn) / WAD;
        uint256 term2 = ((WAD - ALPHA) * varianceOld) / WAD;
        return term1 + term2;
    }

    /// @notice Compute fee from volatility estimate
    /// @param volatilityWAD Volatility as fraction in WAD (e.g., 0.1e18 = 10%)
    /// @return fee Fee in WAD clamped between MIN_FEE and MAX_FEE
    function _computeFee(
        uint256 volatilityWAD
    ) internal pure returns (uint256) {
        uint256 fee = BASE_FEE + (volatilityWAD * FEE_SCALE) / WAD;
        if (fee > MAX_FEE) fee = MAX_FEE;
        if (fee < MIN_FEE) fee = MIN_FEE;
        return fee;
    }
}
