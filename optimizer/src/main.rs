//! Automated strategy generation and backtesting loop using the `amm-match` CLI.
//!
//! Usage:     cargo run -- [--iterations N] [--simulations N] [--database-url URL] [--output results.csv]

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use primitive_types::U256;
use rand::seq::IndexedRandom;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::time::Instant;

use crate::storage::{StoredStrategyResult, StrategyStorage};
// use crate::strategies::boost_decay;
use crate::strategies::from_file::{SolidityFileParams, TemplateFromFile};

pub mod strategies {
    pub mod boost_decay;
    pub mod from_file;
    pub mod widen_after_big_trades;
}
pub mod storage;

#[derive(Parser)]
#[command(about = "Run automated strategy generation and backtesting loop using amm-match")]
pub struct Args {
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
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    std::fs::create_dir_all(&args.sol_dir)?;
    let mut storage = StrategyStorage::new(&args.database_url).await?;

    let template_paths = vec![
        REPO.join("contracts/src/templates/BoostDecay.sol"),
        // REPO.join("contracts/src/templates/StaticFee.sol"),
        // REPO.join("contracts/src/templates/Variance.sol"),
        // REPO.join("contracts/src/templates/Volatility.sol"),
        REPO.join("contracts/src/templates/Volatility2.sol"),
        // REPO.join("contracts/src/templates/WidenAfterBigTrades.sol"),
    ];
    let mut templates = Vec::new();
    for template in template_paths {
        let template = TemplateFromFile::parse_file(&template)?;
        templates.push(template);
    }

    refine_from_seeds(&templates, &mut storage, &args).await?;
    refine_strategies_params(&templates, &mut storage, &args).await?;
    exec_random_strategy_params(&templates, &mut storage, &args).await?;

    Ok(())
}

// -----------------------------------------------------------------------------
// Running a single strategy via `amm-match`
// -----------------------------------------------------------------------------

impl TemplateFromFile {
    /// Run a single strategy: generate Solidity, call amm-match, and return results.
    fn run_with_params(
        &self,
        params: &SolidityFileParams,
        n_simulations: u64,
        sol_dir: &Path,
    ) -> Result<StrategyResult> {
        let source_code = self.generate_solidity(params);

        // Write solidity to a temp file
        let sol_path = sol_dir.join(format!("{}_{}.sol", self.template_name(), params.id()));
        std::fs::write(&sol_path, &source_code)
            .with_context(|| format!("Failed to write Solidity file: {:?}", sol_path))?;

        // Run simulation, get edge
        let edge = match run_amm_match(&sol_path, n_simulations) {
            Ok(edge) => edge,
            Err(e) => {
                return Err(e);
            }
        };

        Ok(StrategyResult {
            template_name: self.template_name().to_string(),
            params: params.clone(),
            edge,
        })
    }

    /// Try to run a strategy with parameters, checking if already processed and storing results.
    #[allow(async_fn_in_trait)]
    async fn run_dedup(
        &self,
        storage: &mut StrategyStorage,
        params: &SolidityFileParams,
        args: &Args,
    ) -> anyhow::Result<StrategyResult> {
        let template_name = self.template_name();

        if let Ok(already_processed) = storage.get_one_exact(&template_name, params).await {
            println!(
                "Skipping already processed: {}_{}",
                template_name,
                params.name_suffix()
            );
            let mut loaded_results = already_processed.parse_into::<SolidityFileParams>()?;
            for (key, loaded_val) in loaded_results.params.values.iter_mut() {
                // copy range into loaded_params
                if let Some(constant) = self.constants.get(key) {
                    loaded_val.range = constant.range.clone();
                }
            }
            return Ok(loaded_results);
        }

        let attempt_id = format!("{}_{}", template_name, params.name_suffix());
        let start = Instant::now();
        match self.run_with_params(params, args.simulations, &args.sol_dir) {
            Ok(result) => {
                let elapsed = start.elapsed();
                println!(
                    "Trying {} with params {}:  Edge: {:.2} (in {:.2?})",
                    self.template_name(),
                    params,
                    result.edge,
                    elapsed
                );
                storage
                    .store_result(&result)
                    .await
                    .context("Failed to store result in database")?;
                Ok(result)
            }
            Err(e) => Err(e.context(format!("Failed in strategy {}", attempt_id))),
        }
    }
}

// Result of a single strategy backtest.
#[derive(Debug, Clone)]
pub struct StrategyResult {
    pub template_name: String,
    pub params: SolidityFileParams,
    pub edge: f64,
}

const SEED_RESULTS: &str = include_str!("../seeds.json");

#[derive(Deserialize)]
struct OptimizerSeed {
    result_id: i64,
    template_name: String,
    recorded_edge: f64,
    params: BTreeMap<String, String>,
}

impl OptimizerSeed {
    fn strategy_params(&self, template: &TemplateFromFile) -> Result<SolidityFileParams> {
        let mut values = template.constants.clone();
        for (name, value) in &self.params {
            let constant = values
                .get_mut(name)
                .with_context(|| format!("Unknown constant {name} in seed {}", self.result_id))?;
            constant.generated_value =
                Some(U256::from_dec_str(value).with_context(|| {
                    format!("Invalid value for {name} in seed {}", self.result_id)
                })?);
        }
        Ok(SolidityFileParams { values })
    }
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

async fn exec_random_strategy_params(
    templates: &[TemplateFromFile],
    storage: &mut StrategyStorage,
    args: &Args,
) -> Result<()> {
    let mut processed_count = 0;
    let mut rng = rand::rng();
    loop {
        let template = templates
            .choose(&mut rng)
            .ok_or(anyhow!("Failed choosing a template"))?;
        let params = template.random_params();

        // If iterations limit is reached, stop
        if let Some(limit) = args.iterations
            && processed_count >= limit
        {
            println!("Reached iteration limit ({}). Stopping.", limit);
            break;
        }

        match template.run_dedup(storage, &params, args).await {
            Ok(_result) => processed_count += 1,
            Err(e) => return Err(e),
        }
    }

    // for params in strategy.params_iter() {
    //     let params_str = params.id();
    // }

    println!("Done. Processed {} new strategies.", processed_count);
    Ok(())
}

async fn refine_strategies_params(
    templates: &[TemplateFromFile],
    storage: &mut StrategyStorage,
    args: &Args,
) -> anyhow::Result<()> {
    let mut template_names = Vec::new();
    for template in templates {
        template_names.push(template.template_name());
    }

    for template in templates {
        let top10: Vec<StoredStrategyResult> =
            storage.best_n(10, Some(&template.template_name())).await?;
        let mut parsed_top10 = Vec::<StrategyResult>::new();
        for result in top10 {
            parsed_top10.push(result.parse_into::<SolidityFileParams>()?);
        }

        for top10_result in &parsed_top10 {
            dbg!(&top10_result.params.to_string(), &top10_result.edge);
            if top10_result.edge < 390.0 {
                continue;
            }
            refine_strategy_params(template, storage, args, top10_result.clone()).await?;
        }
    }

    Ok(())
}

async fn refine_from_seeds(
    templates: &[TemplateFromFile],
    storage: &mut StrategyStorage,
    args: &Args,
) -> Result<()> {
    let seeds: Vec<OptimizerSeed> =
        serde_json::from_str(SEED_RESULTS).context("Failed to read optimizer seeds")?;
    for seed in seeds {
        let template = templates
            .iter()
            .find(|template| template.template_name() == seed.template_name);
        let seed_template;
        let template = match template {
            Some(template) => template,
            None => {
                let path = REPO
                    .join("contracts/src/templates")
                    .join(format!("{}.sol", seed.template_name));
                seed_template = TemplateFromFile::parse_file(&path).with_context(|| {
                    format!("Failed to load template for seed {}", seed.result_id)
                })?;
                &seed_template
            }
        };
        let params = seed.strategy_params(template)?;
        let result = template
            .run_with_params(&params, args.simulations, &args.sol_dir)
            .with_context(|| format!("Failed to evaluate seed {}", seed.result_id))?;
        println!(
            "Seed {} ({}) edge {:.2}, recorded {:.2}",
            seed.result_id, seed.template_name, result.edge, seed.recorded_edge
        );
        refine_strategy_params(template, storage, args, result).await?;
    }
    Ok(())
}

async fn refine_strategy_params(
    template: &TemplateFromFile,
    storage: &mut StrategyStorage,
    args: &Args,
    mut current_result: StrategyResult,
) -> Result<()> {
    for _ in 0..1000 {
        let params = current_result.params.vary_one(&template.constants)?;
        let new_result = template.run_dedup(storage, &params, args).await?;
        if new_result.edge >= current_result.edge {
            current_result = new_result;
        }
    }
    Ok(())
}

pub static REPO: LazyLock<PathBuf> = LazyLock::new(|| {
    let path_bytes = Command::new("git")
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()
        .unwrap()
        .stdout;
    let path_str = str::from_utf8(&path_bytes).unwrap().trim();
    PathBuf::from(path_str)
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_seeds_can_be_refined() -> Result<()> {
        let seeds: Vec<OptimizerSeed> = serde_json::from_str(SEED_RESULTS)?;
        assert_eq!(seeds.len(), 20);
        assert!(seeds.windows(2).all(|pair| {
            pair[0].recorded_edge > pair[1].recorded_edge
                || (pair[0].recorded_edge == pair[1].recorded_edge
                    && pair[0].result_id < pair[1].result_id)
        }));
        for seed in seeds {
            let path = REPO
                .join("contracts/src/templates")
                .join(format!("{}.sol", seed.template_name));
            let template = TemplateFromFile::parse_file(&path)?;
            let params = seed.strategy_params(&template)?;
            for name in seed.params.keys() {
                let constant = &params.values[name];
                let value = constant.generated_value.context("Missing value")?;
                let range = constant.range.as_ref().context("Missing range")?;
                assert!((range.min..=range.max).contains(&value), "{name}: {value}");
            }
            params.vary_one(&template.constants)?;
        }
        Ok(())
    }

    #[tokio::test]
    #[ignore = "utility test"]
    async fn list_top_20() {
        let mut storage =
            StrategyStorage::new(&format!("turso:{}/.cache/db.sqlite", REPO.display()))
                .await
                .expect("Failed to open real database");

        let top = storage.best_n(20, None).await.unwrap();
        assert!(!top.is_empty(), "No results found in the database");

        println!("Fetched {} best results:", top.len());
        for (i, res) in top.iter().enumerate() {
            println!(
                "{}. {} {} -> {:.6}",
                i + 1,
                res.template_name,
                res.params,
                res.edge
            );
        }
    }
}
