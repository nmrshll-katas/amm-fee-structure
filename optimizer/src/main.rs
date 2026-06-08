//! Automated strategy generation and backtesting loop using the `amm-match` CLI.
//!
//! Usage:
//!     cargo run -- [--iterations N] [--simulations N] [--output results.csv]
//!                [--base-fee-min 20] [--base-fee-max 40] [--base-fee-step 5]
//!                [--widen-min 5] [--widen-max 15] [--widen-step 5]
//!                [--decay-min 1] [--decay-max 2] [--decay-step 1]

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use csv::WriterBuilder;
use std::fs::{File, OpenOptions};
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
    pub edge: f64,
    pub params: String,
}

/// Parse the output of `amm-match run`.
fn parse_amm_output(stdout: &str) -> Result<f64> {
    for line in stdout.lines() {
        if let Some(idx) = line.find("Edge:") {
            let after = &line[idx + 5..];
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

    Ok(StrategyResult {
        strategy_name: strategy_name.to_string(),
        edge,
        params: format!(
            "base_fee={}bps,widen={}bps,decay={}bps",
            params.base_fee_bps, params.widen_threshold_bps, params.decay_rate_bps
        ),
    })
}

// -----------------------------------------------------------------------------
// Parameter grid generation
// -----------------------------------------------------------------------------

/// Generate all combinations of parameters from the given ranges.
fn generate_param_grid(
    base_min: u64,
    base_max: u64,
    base_step: u64,
    widen_min: u64,
    widen_max: u64,
    widen_step: u64,
    decay_min: u64,
    decay_max: u64,
    decay_step: u64,
) -> Vec<StrategyParams> {
    let mut combos = Vec::new();
    for base_fee in (base_min..=base_max).step_by(base_step as usize) {
        for widen in (widen_min..=widen_max).step_by(widen_step as usize) {
            for decay in (decay_min..=decay_max).step_by(decay_step as usize) {
                let name_suffix = format!("b{}_w{}_d{}", base_fee, widen, decay);
                combos.push(StrategyParams {
                    base_fee_bps: base_fee,
                    widen_threshold_bps: widen,
                    decay_rate_bps: decay,
                    name_suffix,
                });
            }
        }
    }
    combos
}

// -----------------------------------------------------------------------------
// CSV append helper
// -----------------------------------------------------------------------------

/// Append a single result to the CSV file. Write header if file does not exist or is empty.
fn append_result_to_csv(output_path: &Path, result: &StrategyResult) -> Result<()> {
    let file_exists = output_path.exists();
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(output_path)?;
    let mut wtr = WriterBuilder::new().from_writer(file);

    if !file_exists || output_path.metadata()?.len() == 0 {
        wtr.write_record(&["strategy_name", "edge", "params"])?;
    }

    wtr.write_record(&[
        &result.strategy_name,
        &result.edge.to_string(),
        &result.params,
    ])?;
    wtr.flush()?;
    Ok(())
}

// -----------------------------------------------------------------------------
// CLI arguments and main
// -----------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    author,
    version,
    about = "Run automated strategy generation and backtesting loop using amm-match"
)]
struct Args {
    /// Number of strategy variations to test (default: all combos)
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

    // Parameter ranges for base fee (bps)
    #[arg(long, default_value_t = 20)]
    base_fee_min: u64,
    #[arg(long, default_value_t = 40)]
    base_fee_max: u64,
    #[arg(long, default_value_t = 5)]
    base_fee_step: u64,

    // Parameter ranges for widen threshold (bps)
    #[arg(long, default_value_t = 5)]
    widen_min: u64,
    #[arg(long, default_value_t = 15)]
    widen_max: u64,
    #[arg(long, default_value_t = 5)]
    widen_step: u64,

    // Parameter ranges for decay rate (bps)
    #[arg(long, default_value_t = 1)]
    decay_min: u64,
    #[arg(long, default_value_t = 2)]
    decay_max: u64,
    #[arg(long, default_value_t = 1)]
    decay_step: u64,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Create the directory for Solidity files
    std::fs::create_dir_all(&args.sol_dir)?;

    let all_params = generate_param_grid(
        args.base_fee_min,
        args.base_fee_max,
        args.base_fee_step,
        args.widen_min,
        args.widen_max,
        args.widen_step,
        args.decay_min,
        args.decay_max,
        args.decay_step,
    );

    if all_params.is_empty() {
        anyhow::bail!("No parameter combinations generated (check min/max/step values)");
    }

    let n_iter = args.iterations.unwrap_or(all_params.len());
    let params_to_test = &all_params[..n_iter.min(all_params.len())];

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
                println!("  Edge: {:.2} (in {:.2?})", result.edge, elapsed);
                // Append result to CSV immediately
                if let Err(e) = append_result_to_csv(&args.output, &result) {
                    eprintln!("  WARNING: Failed to append to CSV: {}", e);
                }
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

    println!("\nResults appended to: {:?}", args.output);
    Ok(())
}
