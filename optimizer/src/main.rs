//! Automated strategy generation and backtesting loop using the `amm-match` CLI.
//!
//! Usage:     cargo run -- [--iterations N] [--simulations N] [--database-url URL] [--output results.csv]

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use crate::storage::StrategyStorage;
use crate::strategies::boost_decay_2;

pub mod strategies {
    pub mod boost_decay_2;
    pub mod boost_decay_example;
}
pub mod storage;

#[tokio::main]
async fn main() -> Result<()> {
    #[derive(Parser)]
    #[command(about = "Run automated strategy generation and backtesting loop using amm-match")]
    struct Args {
        /// Number of new strategy variations to test (default: all unseen combos)
        #[arg(long)]
        iterations: Option<usize>,

        /// Number of simulations per strategy
        #[arg(long, default_value_t = 10)]
        simulations: u64,

        /// Output CSV file path (kept for backward compatibility, but storage is preferred)
        #[arg(long, default_value = ".cache/strategy_results.csv")]
        output: PathBuf,

        /// Directory to store generated Solidity files
        #[arg(long, default_value = ".cache/generated_strategies")]
        sol_dir: PathBuf,

        /// Turso database URL (e.g., "turso://...", "turso::memory:", or file path)
        #[arg(long, default_value = "turso:.cache/db.sqlite")]
        database_url: String,
    }
    let args = Args::parse();

    std::fs::create_dir_all(&args.sol_dir)?;
    let mut storage = StrategyStorage::new(&args.database_url).await?;

    pub enum LoopControl {
        Next,
        // Done,
    }
    async fn try_strategy_params<S: StrategyTemplate>(
        strategy: &S,
        storage: &mut StrategyStorage,
        params: &S::Params,
        args: &Args,
    ) -> anyhow::Result<LoopControl> {
        let template_name = strategy.template_name();
        let params_str = params.to_params_string();

        // Check if already processed
        if storage.exists(template_name, &params_str).await? {
            println!(
                "Skipping already processed: {}_{}",
                template_name, params_str
            );
            return Ok(LoopControl::Next);
        }

        let attempt_id = format!("{}_{}", template_name, params.name_suffix());
        let start = Instant::now();
        match strategy.run_with_params(params, args.simulations, &args.sol_dir) {
            Ok(result) => {
                let elapsed = start.elapsed();
                println!("  Edge: {:.2} (in {:.2?})", result.edge, elapsed);
                storage
                    .store_result(&result)
                    .await
                    .context("Failed to store result in database")?;
                Ok(LoopControl::Next)
            }
            Err(e) => Err(e.context(format!("Failed in strategy {}", attempt_id))),
        }
    }

    let mut processed_count = 0;
    let strategy = boost_decay_2::Strategy {};
    loop {
        let params = boost_decay_2::StrategyParams::sample_random();

        // If iterations limit is reached, stop
        if let Some(limit) = args.iterations
            && processed_count >= limit
        {
            println!("Reached iteration limit ({}). Stopping.", limit);
            break;
        }

        match try_strategy_params(&strategy, &mut storage, &params, &args).await {
            Ok(LoopControl::Next) => processed_count += 1,
            Err(e) => return Err(e),
        }
    }

    // for params in strategy.params_iter() {
    //     let params_str = params.to_params_string();
    // }

    println!("Done. Processed {} new strategies.", processed_count);
    Ok(())
}

// -----------------------------------------------------------------------------
// Running a single strategy via `amm-match`
// -----------------------------------------------------------------------------

/// A strategy template that can generate Solidity code from parameters.
pub trait StrategyTemplate {
    /// The type of parameters this template uses.
    type Params: IsParams;

    /// Unique name of the template (for CSV and identification).
    fn template_name(&self) -> &'static str;

    /// Generate the full Solidity source for a given parameter set and strategy name.
    fn generate_solidity(&self, params: &Self::Params) -> String;

    /// Generate all possible parameter combinations defined for this template.
    fn params_iter(&self) -> Box<dyn Iterator<Item = Self::Params>>;

    /// Run a single strategy: generate Solidity, call amm-match, and return results.
    fn run_with_params(
        &self,
        params: &Self::Params,
        n_simulations: u64,
        sol_dir: &Path,
    ) -> Result<StrategyResult> {
        let source_code = self.generate_solidity(params);

        // Write solidity to a temp file
        let sol_path = sol_dir.join(format!(
            "{}_{}.sol",
            self.template_name(),
            params.to_params_string()
        ));
        std::fs::write(&sol_path, &source_code)
            .with_context(|| format!("Failed to write Solidity file: {:?}", sol_path))?;

        // Run simulation, get edge
        let edge = run_amm_match(&sol_path, n_simulations)?;

        Ok(StrategyResult {
            template_name: self.template_name().to_string(),
            params: params.to_params_string(),
            edge,
        })
    }
}
pub trait IsParams: Clone {
    fn sample_random() -> Self;
    fn to_params_string(&self) -> String;
    fn name_suffix(&self) -> String;
}

// Result of a single strategy backtest.
#[derive(Debug)]
pub struct StrategyResult {
    pub template_name: String,
    pub params: String,
    pub edge: f64,
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
