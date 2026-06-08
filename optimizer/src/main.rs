//! Automated strategy generation and backtesting loop using the `amm-match` CLI.
//!
//! Usage:
//!     cargo run -- [--iterations N] [--simulations N] [--output results.csv]

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use csv::Writer;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

// -----------------------------------------------------------------------------
// Parameter definition and Solidity generation
// -----------------------------------------------------------------------------

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

// -----------------------------------------------------------------------------
// Running a single strategy via `amm-match`
// -----------------------------------------------------------------------------

/// Result of a single strategy backtest.
#[derive(Debug)]
pub struct StrategyResult {
    pub strategy_name: String,
    // pub avg_edge: f64,
    pub edge: f64,
    // pub wins: u64,
    // pub losses: u64,
    // pub draws: u64,
    // pub win_rate: f64,
    pub params: String,
}

/// Parse the output of `amm-match run`.
fn parse_amm_output(stdout: &str) -> Result<f64> {
    for line in stdout.lines() {
        // Find the exact "Edge:" token (not "Avg Edge:" etc.)
        if let Some(idx) = line.find("Edge:") {
            // Start after "Edge:"
            let after = &line[idx + 5..];
            // Find the first word/number in the remainder
            let number_str = after
                .split_whitespace()
                .next()
                .ok_or_else(|| anyhow!("No number after 'Edge:' in line: {}", line))?;
            return number_str
                .parse::<f64>()
                .map_err(|_| anyhow!("Failed to parse '{}' as f64 in line: {}", number_str, line));
        }
    }
    Err(anyhow!("No 'Edge:' line found in:\n{}", stdout))
}

/// Run `amm-match run` on a Solidity file.
fn run_amm_match(solidity_path: &Path, n_simulations: u64) -> Result<f64> {
    let mut cmd = Command::new("uv");
    cmd.arg("run")
        .arg("python")
        .arg("-c")
        .arg("from amm_competition.cli import main; main()")
        .arg("run")
        .arg(solidity_path.to_str().unwrap())
        .arg("--simulations")
        .arg(n_simulations.to_string())
        .env("UV_PROJECT_ENVIRONMENT", ".venv")
        .current_dir(".");
    let cmd_str = std::iter::once(cmd.get_program().to_string_lossy())
        .chain(cmd.get_args().map(|a| a.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ");
    println!("Running: {}", cmd_str);
    let output = cmd.output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "amm-match failed with exit code: {}\n{}",
            output.status,
            stderr
        ));
    }

    let stdout = String::from_utf8(output.stdout)?;
    println!("stdout: {}", stdout);
    parse_amm_output(&stdout)
}

/// Run a single strategy: generate Solidity, call amm-match, and return results.
pub fn run_single_strategy(
    params: &StrategyParams,
    strategy_name: &str,
    n_simulations: u64,
    sol_dir: &Path,
) -> Result<StrategyResult> {
    // 1. Generate Solidity source
    let source_code = generate_strategy_solidity(params, strategy_name);

    // 2. Write to a file
    let sol_path = sol_dir.join(format!("{}.sol", strategy_name));
    std::fs::write(&sol_path, &source_code)
        .with_context(|| format!("Failed to write Solidity file: {:?}", sol_path))?;

    // 3. Call amm-match
    let edge = run_amm_match(&sol_path, n_simulations)?;
    // let avg_edge = total_edge / n_simulations as f64;
    // let win_rate = wins as f64 / n_simulations as f64;

    Ok(StrategyResult {
        strategy_name: strategy_name.to_string(),
        // avg_edge,
        edge,
        // wins,
        // losses,
        // draws,
        // win_rate,
        params: format!(
            "base_fee={}bps,widen={}bps,decay={}bps",
            params.base_fee_bps, params.widen_threshold_bps, params.decay_rate_bps
        ),
    })
}

// -----------------------------------------------------------------------------
// Main loop and CSV output
// -----------------------------------------------------------------------------

/// Define the parameter grid (same as the Python example).
fn param_grid() -> Vec<StrategyParams> {
    vec![
        StrategyParams {
            base_fee_bps: 20,
            widen_threshold_bps: 10,
            decay_rate_bps: 1,
            name_suffix: "low_fee_fast_decay".to_string(),
        },
        StrategyParams {
            base_fee_bps: 25,
            widen_threshold_bps: 10,
            decay_rate_bps: 1,
            name_suffix: "low_fee_fast_decay".to_string(),
        },
        StrategyParams {
            base_fee_bps: 30,
            widen_threshold_bps: 10,
            decay_rate_bps: 1,
            name_suffix: "base_fee_fast_decay".to_string(),
        },
        StrategyParams {
            base_fee_bps: 35,
            widen_threshold_bps: 10,
            decay_rate_bps: 1,
            name_suffix: "mid_fee_fast_decay".to_string(),
        },
        StrategyParams {
            base_fee_bps: 40,
            widen_threshold_bps: 10,
            decay_rate_bps: 1,
            name_suffix: "mid_fee_fast_decay".to_string(),
        },
        StrategyParams {
            base_fee_bps: 30,
            widen_threshold_bps: 10,
            decay_rate_bps: 2,
            name_suffix: "base_fee_slow_decay".to_string(),
        },
        StrategyParams {
            base_fee_bps: 30,
            widen_threshold_bps: 5,
            decay_rate_bps: 1,
            name_suffix: "tight_widen_fast_decay".to_string(),
        },
        StrategyParams {
            base_fee_bps: 30,
            widen_threshold_bps: 15,
            decay_rate_bps: 1,
            name_suffix: "loose_widen_fast_decay".to_string(),
        },
    ]
}

#[derive(Parser)]
#[command(
    author,
    version,
    about = "Run automated strategy generation and backtesting loop using amm-match"
)]
struct Args {
    /// Number of strategy variations to test (default: all)
    #[arg(long)]
    iterations: Option<usize>,

    /// Number of simulations per strategy
    #[arg(long, default_value_t = 100)]
    simulations: u64,

    /// Output CSV file path
    #[arg(long, default_value = "strategy_results.csv")]
    output: PathBuf,

    /// Directory to store generated Solidity files
    #[arg(long, default_value = ".cache/generated_strategies")]
    sol_dir: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Create the directory for Solidity files
    std::fs::create_dir_all(&args.sol_dir)?;

    let all_params = param_grid();
    let n_iter = args.iterations.unwrap_or(all_params.len());
    let params_to_test = &all_params[..n_iter.min(all_params.len())];

    let mut results = Vec::new();

    for (i, params) in params_to_test.iter().enumerate() {
        let strategy_name = format!("AutoGen_{}_{}", params.name_suffix, i);
        println!(
            "[{}/{}] Running strategy: {}",
            i + 1,
            params_to_test.len(),
            strategy_name
        );
        println!(
            "  Params: base_fee={}bps, widen={}bps, decay={}bps",
            params.base_fee_bps, params.widen_threshold_bps, params.decay_rate_bps
        );

        let start = Instant::now();
        match run_single_strategy(params, &strategy_name, args.simulations, &args.sol_dir) {
            Ok(result) => {
                let elapsed = start.elapsed();
                println!(
                    "  Edge: {:.2}",
                    result.edge,
                    // result.win_rate * 100.0,
                    // elapsed
                );
                results.push(result);
            }
            Err(e) => {
                eprintln!("  ERROR: {}", e);
                if std::env::var("RUST_BACKTRACE").is_ok() {
                    return Err(e.context(format!("Failed in strategy {}", strategy_name)));
                } else {
                    eprintln!("  ERROR: {}", e);
                    continue; // next strategy
                }
            }
        }
    }

    // Write results to CSV
    let mut wtr = Writer::from_path(&args.output)?;
    wtr.write_record(&[
        "strategy_name",
        // "avg_edge",
        "edge",
        // "wins",
        // "losses",
        // "draws",
        // "win_rate",
        "params",
    ])?;

    for res in &results {
        wtr.write_record(&[
            &res.strategy_name,
            &res.edge.to_string(),
            // &res.total_edge.to_string(),
            // &res.wins.to_string(),
            // &res.losses.to_string(),
            // &res.draws.to_string(),
            // &res.win_rate.to_string(),
            &res.params,
        ])?;
    }
    wtr.flush()?;

    println!("\nResults saved to: {:?}", args.output);

    // if !results.is_empty() {
    //     let best = results
    //         .iter()
    //         .max_by(|a, b| a.avg_edge.partial_cmp(&b.avg_edge).unwrap())
    //         .unwrap();
    //     println!(
    //         "Best strategy: {} with avg edge {:.2}",
    //         best.strategy_name, best.avg_edge
    //     );
    // }

    Ok(())
}
