// use crate::{IsParams, StrategyTemplate};
// use rand::prelude::*;
// use serde::{Deserialize, Serialize};

// /// Strategy parameters for the latent‑state strategy.
// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub struct StrategyParams {
//     pub base_fee_bps: u64,
//     pub decay_fast_bps: u64,
//     pub decay_slow_bps: u64,
//     pub decay_cooldown_bps: u64,
//     pub alpha_spot_bps: u64,
// }
// impl StrategyParams {
//     pub const BASE_FEE_MIN: u64 = 20;
//     pub const BASE_FEE_MAX: u64 = 40;
//     pub const BASE_FEE_STEP: u64 = 1;

//     pub const DECAY_FAST_MIN: u64 = 10;
//     pub const DECAY_FAST_MAX: u64 = 20000;
//     pub const DECAY_FAST_STEP: u64 = 10;

//     pub const DECAY_SLOW_MIN: u64 = 100;
//     pub const DECAY_SLOW_MAX: u64 = 20000;
//     pub const DECAY_SLOW_STEP: u64 = 100;

//     pub const DECAY_COOLDOWN_MIN: u64 = 1000;
//     pub const DECAY_COOLDOWN_MAX: u64 = 20000;
//     pub const DECAY_COOLDOWN_STEP: u64 = 100;

//     pub const ALPHA_SPOT_MIN: u64 = 1;
//     pub const ALPHA_SPOT_MAX: u64 = 100;
//     pub const ALPHA_SPOT_STEP: u64 = 1;
// }
// impl IsParams<Template> for StrategyParams {
//     /// Convert the parameters into a stable, parseable string for CSV tracking.
//     fn id(&self) -> String {
//         format!(
//             "base_fee={}bps|decay_fast={}bps|decay_slow={}bps|decay_cooldown={}bps|alpha_spot={}bps",
//             self.base_fee_bps,
//             self.decay_fast_bps,
//             self.decay_slow_bps,
//             self.decay_cooldown_bps,
//             self.alpha_spot_bps
//         )
//     }
//     /// Generate a human‑readable name suffix (e.g., "b10_dF8500_dS9200_dC9000_a12").
//     fn name_suffix(&self) -> String {
//         format!(
//             "b{}_dF{}_dS{}_dC{}_a{}",
//             self.base_fee_bps,
//             self.decay_fast_bps,
//             self.decay_slow_bps,
//             self.decay_cooldown_bps,
//             self.alpha_spot_bps
//         )
//     }

//     fn sample_random(_: &Template) -> StrategyParams {
//         let mut rng = rand::rng();
//         StrategyParams {
//             base_fee_bps: rng.random_range(0..=StrategyParams::BASE_FEE_MAX),
//             decay_fast_bps: rng.random_range(0..=StrategyParams::DECAY_FAST_MAX),
//             decay_slow_bps: rng.random_range(0..=StrategyParams::DECAY_SLOW_MAX),
//             decay_cooldown_bps: rng.random_range(0..=StrategyParams::DECAY_COOLDOWN_MAX),
//             alpha_spot_bps: rng.random_range(0..=StrategyParams::ALPHA_SPOT_MAX),
//         }
//     }
// }
// impl std::fmt::Display for StrategyParams {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         writeln!(
//             f,
//             "StrategyParams: baseFee={} decayFast={} decaySlow={} decayCooldown={} alphaSpot={}",
//             self.base_fee_bps,
//             self.decay_fast_bps,
//             self.decay_slow_bps,
//             self.decay_cooldown_bps,
//             self.alpha_spot_bps,
//         )
//     }
// }

// pub struct Template {}
// impl StrategyTemplate for Template {
//     type Params = StrategyParams;

//     fn template_name(&self) -> String {
//         "boost_decay_2".to_string()
//     }

//     fn generate_solidity(&self, params: &Self::Params) -> String {
//         /// The Solidity template with {{placeholders}}.
//         const TEMPLATE: &str = r#"// SPDX-License-Identifier: MIT
//           pragma solidity ^0.8.24;

//           import {AMMStrategyBase} from "./AMMStrategyBase.sol";
//           import {TradeInfo} from "./IAMMStrategy.sol";

//           /// @title BoostOnShockThenDecay
//           /// @notice Preserve calmer resting fees, but charge more aggressively during real shocks.
//           contract Strategy is AMMStrategyBase {
//               uint256 internal constant BASE_FEE = {{BASE_FEE}} * BPS;
//               uint256 internal constant DECAY_FAST = {{DECAY_FAST}} * BPS;
//               uint256 internal constant DECAY_SLOW = {{DECAY_SLOW}} * BPS;
//               uint256 internal constant DECAY_COOLDOWN = {{DECAY_COOLDOWN}} * BPS;
//               uint256 internal constant ALPHA_SPOT = {{ALPHA_SPOT}} * BPS;

//               function afterInitialize(
//                   uint256 initialX,
//                   uint256 initialY
//               ) external override returns (uint256 bidFee, uint256 askFee) {
//                   uint256 spot = wdiv(initialY, initialX);

//                   slots[0] = spot; // latent spot estimate
//                   slots[1] = 0; // bid pressure
//                   slots[2] = 0; // ask pressure
//                   slots[3] = 0; // cooldown
//                   slots[4] = 0; // size memory
//                   slots[5] = 0; // last side: 1 buy, 2 sell
//                   slots[6] = spot; // last observed spot
//                   slots[7] = 0; // last timestamp

//                   return (BASE_FEE, BASE_FEE);
//               }

//               function afterSwap(
//                   TradeInfo calldata trade
//               ) external override returns (uint256 bidFee, uint256 askFee) {
//                   uint256 currentSpot = wdiv(trade.reserveY, trade.reserveX);
//                   uint256 latentSpot = _blend(slots[0], currentSpot, ALPHA_SPOT);
//                   uint256 lastSpot = slots[6];
//                   uint256 lastTimestamp = slots[7];
//                   uint256 gap = trade.timestamp > lastTimestamp
//                       ? trade.timestamp - lastTimestamp
//                       : 0;

//                   uint256 sizeX = wdiv(trade.amountX, trade.reserveX);
//                   uint256 sizeY = wdiv(trade.amountY, trade.reserveY);
//                   uint256 tradeSize = sizeX > sizeY ? sizeX : sizeY;

//                   uint256 spotJump = lastSpot == 0
//                       ? 0
//                       : wdiv(absDiff(currentSpot, lastSpot), lastSpot);
//                   uint256 bidPressure = wmul(slots[1], DECAY_FAST);
//                   uint256 askPressure = wmul(slots[2], DECAY_FAST);
//                   uint256 cooldown = wmul(slots[3], DECAY_COOLDOWN);
//                   uint256 sizeMemory = wmul(slots[4], DECAY_SLOW);

//                   if (gap >= 4) {
//                       bidPressure = wmul(bidPressure, 8000 * BPS);
//                       askPressure = wmul(askPressure, 8000 * BPS);
//                       cooldown = wmul(cooldown, 7400 * BPS);
//                       sizeMemory = wmul(sizeMemory, 8400 * BPS);
//                   } else if (gap >= 2) {
//                       bidPressure = wmul(bidPressure, 9000 * BPS);
//                       askPressure = wmul(askPressure, 9000 * BPS);
//                       cooldown = wmul(cooldown, 8800 * BPS);
//                       sizeMemory = wmul(sizeMemory, 9300 * BPS);
//                   }

//                   uint256 shock = tradeSize > spotJump ? tradeSize : spotJump;
//                   if (tradeSize > 10 * BPS) {
//                       shock += wmul(tradeSize, 2000 * BPS);
//                   } else if (tradeSize > 4 * BPS) {
//                       shock += wmul(tradeSize, 1200 * BPS);
//                   }

//                   cooldown = clamp(cooldown + wmul(shock, 1500 * BPS), 0, WAD);
//                   sizeMemory = sizeMemory > tradeSize ? sizeMemory : tradeSize;

//                   if (trade.isBuy) {
//                       bidPressure = clamp(bidPressure + wmul(shock, 1600 * BPS), 0, WAD);
//                       askPressure = clamp(askPressure + wmul(shock, 700 * BPS), 0, WAD);
//                   } else {
//                       askPressure = clamp(askPressure + wmul(shock, 1600 * BPS), 0, WAD);
//                       bidPressure = clamp(bidPressure + wmul(shock, 700 * BPS), 0, WAD);
//                   }

//                   uint256 common = clampFee(
//                       BASE_FEE + wmul(cooldown, 2350 * BPS) + wmul(sizeMemory, 880 * BPS)
//                   );

//                   if (gap >= 3 && cooldown < 5 * BPS) {
//                       common = common > 2 * BPS ? common - 2 * BPS : MIN_FEE;
//                   }

//                   bidFee =
//                       common +
//                       wmul(bidPressure, 2200 * BPS) +
//                       wmul(shock, trade.isBuy ? 1800 * BPS : 600 * BPS);
//                   askFee =
//                       common +
//                       wmul(askPressure, 2200 * BPS) +
//                       wmul(shock, trade.isBuy ? 600 * BPS : 1800 * BPS);

//                   uint256 eventSignal = shock > spotJump ? shock : spotJump;
//                   if (eventSignal > 5 * BPS) {
//                       uint256 special = wmul(eventSignal - 5 * BPS, 650 * BPS);
//                       if (trade.isBuy) {
//                           bidFee += special;
//                           askFee += special / 4;
//                       } else {
//                           askFee += special;
//                           bidFee += special / 4;
//                       }
//                   }

//                   if (currentSpot >= latentSpot) {
//                       bidFee = clampFee(bidFee + BPS);
//                   } else {
//                       askFee = clampFee(askFee + BPS);
//                   }

//                   if (trade.isBuy) {
//                       if (slots[5] == 1) {
//                           bidFee = clampFee(bidFee + 2 * BPS);
//                       }
//                       if (cooldown < 5 * BPS && askFee > 5 * BPS) {
//                           askFee -= 5 * BPS;
//                       } else if (cooldown < 7 * BPS && askFee > 3 * BPS) {
//                           askFee -= 3 * BPS;
//                       }
//                       slots[5] = 1;
//                   } else {
//                       if (slots[5] == 2) {
//                           askFee = clampFee(askFee + 2 * BPS);
//                       }
//                       if (cooldown < 5 * BPS && bidFee > 5 * BPS) {
//                           bidFee -= 5 * BPS;
//                       } else if (cooldown < 7 * BPS && bidFee > 3 * BPS) {
//                           bidFee -= 3 * BPS;
//                       }
//                       slots[5] = 2;
//                   }

//                   bidFee = clampFee(bidFee);
//                   askFee = clampFee(askFee);

//                   slots[0] = latentSpot;
//                   slots[1] = bidPressure;
//                   slots[2] = askPressure;
//                   slots[3] = cooldown;
//                   slots[4] = sizeMemory;
//                   slots[6] = currentSpot;
//                   slots[7] = trade.timestamp;

//                   return (bidFee, askFee);
//               }

//               function getName() external pure override returns (string memory) {
//                   return "{{STRATEGY_NAME}}";
//               }

//               function _blend(
//                   uint256 prev,
//                   uint256 sample,
//                   uint256 alpha
//               ) internal pure returns (uint256) {
//                   return wmul(prev, WAD - alpha) + wmul(sample, alpha);
//               }
//           }
//           "#;

//         TEMPLATE
//             .replace("{{BASE_FEE}}", &params.base_fee_bps.to_string())
//             .replace("{{DECAY_FAST}}", &params.decay_fast_bps.to_string())
//             .replace("{{DECAY_SLOW}}", &params.decay_slow_bps.to_string())
//             .replace("{{DECAY_COOLDOWN}}", &params.decay_cooldown_bps.to_string())
//             .replace("{{ALPHA_SPOT}}", &params.alpha_spot_bps.to_string())
//             .replace("{{STRATEGY_NAME}}", &self.template_name())
//     }

//     fn params_iter(&self) -> Box<dyn Iterator<Item = Self::Params>> {
//         Box::new(ParamGridIterator::default())
//     }

//     fn random_params(&self) -> Self::Params {
//         Self::Params::sample_random(self)
//     }
// }

// pub struct ParamGridIterator {
//     base: u64,
//     decay_fast: u64,
//     decay_slow: u64,
//     decay_cooldown: u64,
//     alpha: u64,
//     base_done: bool,
// }
// impl Default for ParamGridIterator {
//     fn default() -> Self {
//         ParamGridIterator {
//             base: StrategyParams::BASE_FEE_MIN,
//             decay_fast: StrategyParams::DECAY_FAST_MIN,
//             decay_slow: StrategyParams::DECAY_SLOW_MIN,
//             decay_cooldown: StrategyParams::DECAY_COOLDOWN_MIN,
//             alpha: StrategyParams::ALPHA_SPOT_MIN,
//             base_done: false,
//         }
//     }
// }
// impl Iterator for ParamGridIterator {
//     type Item = StrategyParams;
//     fn next(&mut self) -> Option<Self::Item> {
//         if self.base_done {
//             return None;
//         }
//         // Return the current combination
//         let result = StrategyParams {
//             base_fee_bps: self.base,
//             decay_fast_bps: self.decay_fast,
//             decay_slow_bps: self.decay_slow,
//             decay_cooldown_bps: self.decay_cooldown,
//             alpha_spot_bps: self.alpha,
//         };
//         // Advance state
//         self.alpha += StrategyParams::ALPHA_SPOT_STEP;
//         if self.alpha > StrategyParams::ALPHA_SPOT_MAX {
//             self.alpha = StrategyParams::ALPHA_SPOT_MIN;
//             self.decay_cooldown += StrategyParams::DECAY_COOLDOWN_STEP;
//             if self.decay_cooldown > StrategyParams::DECAY_COOLDOWN_MAX {
//                 self.decay_cooldown = StrategyParams::DECAY_COOLDOWN_MIN;
//                 self.decay_slow += StrategyParams::DECAY_SLOW_STEP;
//                 if self.decay_slow > StrategyParams::DECAY_SLOW_MAX {
//                     self.decay_slow = StrategyParams::DECAY_SLOW_MIN;
//                     self.decay_fast += StrategyParams::DECAY_FAST_STEP;
//                     if self.decay_fast > StrategyParams::DECAY_FAST_MAX {
//                         self.decay_fast = StrategyParams::DECAY_FAST_MIN;
//                         self.base += StrategyParams::BASE_FEE_STEP;
//                         if self.base > StrategyParams::BASE_FEE_MAX {
//                             self.base_done = true;
//                         }
//                     }
//                 }
//             }
//         }
//         Some(result)
//     }
// }
