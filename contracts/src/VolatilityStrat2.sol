// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {AMMStrategyBase} from "./AMMStrategyBase.sol";
import {IAMMStrategy, TradeInfo} from "./IAMMStrategy.sol";

contract Strategy is AMMStrategyBase {
    // -------- Configuration constants (WAD precision) --------
    uint256 public constant BASE_FEE = 20 * BPS; // 20 bps = 0.002
    uint256 public constant VOL_MULTIPLIER = 5e16; // 0.05 WAD → 5 bps per 1% vol
    uint256 public constant IMB_FACTOR = 1e16; // 0.01 WAD → 1 bps per 10% imbalance

    uint256 private constant WINDOW = 8; // number of prices for vol estimation

    // -------- Storage layout (slots are free, we use them explicitly) --------
    // slots[0] = initialX
    // slots[1] = initialY
    // slots[2] = number of stored prices (max WINDOW)
    // slots[3..10] = ring buffer of last WINDOW prices (slot3 = price[0], slot4 = price[1], ...)
    // (slots[3] is used as price[0] because slots[0..2] are taken)

    function afterInitialize(
        uint256 initialX,
        uint256 initialY
    ) external returns (uint256 bidFee, uint256 askFee) {
        // Store initial reserves permanently
        writeSlot(0, initialX);
        writeSlot(1, initialY);

        // Compute initial price = reserveY / reserveX (WAD)
        uint256 initialPrice = wdiv(initialY, initialX);

        // Initialise price buffer: only one price stored
        writeSlot(2, 1); // count = 1
        writeSlot(3, initialPrice); // price[0]

        // Start with symmetric base fee
        return (BASE_FEE, BASE_FEE);
    }

    function afterSwap(
        TradeInfo calldata trade
    ) external returns (uint256 bidFee, uint256 askFee) {
        // ----- 1. Read state from storage -----
        uint256 initialX = readSlot(0);
        uint256 initialY = readSlot(1);
        uint256 count = readSlot(2); // number of stored prices (≤ WINDOW)

        uint256 reserveX = trade.reserveX;
        uint256 reserveY = trade.reserveY;

        // ----- 2. Update price buffer with the new post‑swap price -----
        uint256 newPrice = wdiv(reserveY, reserveX);
        if (count < WINDOW) {
            // Append at the end
            writeSlot(3 + count, newPrice);
            writeSlot(2, count + 1);
        } else {
            // Buffer is full: shift left (drop oldest, append newest)
            for (uint256 i = 3; i < 3 + WINDOW - 1; i++) {
                writeSlot(i, readSlot(i + 1));
            }
            writeSlot(3 + WINDOW - 1, newPrice);
        }

        // Updated count (might have increased or stayed at WINDOW)
        uint256 newCount = readSlot(2);

        // ----- 3. Compute volatility (average absolute % change over last trades) -----
        uint256 volatility = 0;
        if (newCount >= 2) {
            uint256 sumAbsReturns = 0;
            uint256 pairs = newCount - 1;
            for (uint256 i = 0; i < pairs; i++) {
                uint256 pricePrev = readSlot(3 + i);
                uint256 priceCurr = readSlot(3 + i + 1);
                uint256 absChange = priceCurr > pricePrev
                    ? priceCurr - pricePrev
                    : pricePrev - priceCurr;
                // absolute return = abs(Δprice) / pricePrev  (WAD)
                uint256 ret = wdiv(absChange, pricePrev);
                sumAbsReturns += ret;
            }
            volatility = sumAbsReturns / pairs; // average absolute return (WAD)
        }

        // ----- 4. Base fee + volatility adjustment -----
        uint256 volComponent = wmul(VOL_MULTIPLIER, volatility);
        uint256 baseVol = BASE_FEE + volComponent;
        if (baseVol > MAX_FEE) baseVol = MAX_FEE;

        // ----- 5. Inventory imbalance adjustment -----
        // imbalance ratio = |reserveX - initialX| / initialX   (WAD)
        uint256 diffX = reserveX > initialX
            ? reserveX - initialX
            : initialX - reserveX;
        uint256 imbalanceRatio = wdiv(diffX, initialX);
        uint256 imbalAdj = wmul(IMB_FACTOR, imbalanceRatio);

        // Direction: if reserveX > initialX → excess X → want to sell X (lower askFee)
        //            if reserveX < initialX → excess Y → want to sell Y (lower bidFee)
        if (reserveX > initialX) {
            // Excess X
            askFee = baseVol > imbalAdj ? baseVol - imbalAdj : 0;
            bidFee = baseVol + imbalAdj;
        } else if (reserveX < initialX) {
            // Excess Y
            bidFee = baseVol > imbalAdj ? baseVol - imbalAdj : 0;
            askFee = baseVol + imbalAdj;
        } else {
            // Balanced
            bidFee = baseVol;
            askFee = baseVol;
        }

        // ----- 6. Clamp to allowed range and return -----
        bidFee = clampFee(bidFee);
        askFee = clampFee(askFee);
        return (bidFee, askFee);
    }

    function getName() external pure returns (string memory) {
        return "Volatility & Inventory Strategy";
    }
}
