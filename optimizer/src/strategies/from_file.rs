use anyhow::Context;
use primitive_types::U256;
use rand::{RngExt, rngs::ThreadRng, seq::IndexedRandom};
use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    collections::HashMap,
    fs,
    iter::Iterator,
    path::{Path, PathBuf},
};

/// Holds the original Solidity source and a map of all parsed constants.
pub struct TemplateFromFile {
    /// Full original Solidity source code.
    pub original: String,
    /// Map from constant name to its definition.
    pub constants: HashMap<String, ConstantDef>,
    /// Original file path (if loaded from file)
    pub file_path: Option<PathBuf>,
}
impl TemplateFromFile {
    /// Parses the given Solidity source, extracting constants that match
    /// `uint256 internal constant`. Returns `None` if the source could not be parsed.
    pub fn parse_str(source: &str) -> anyhow::Result<Self> {
        let mut constants = HashMap::new();
        for line in source.lines() {
            if let Ok(def) = ConstantDef::parse_str(line) {
                constants.insert(def.name.clone(), def);
            }
        }

        Ok(Self {
            original: source.to_string(),
            constants,
            file_path: None,
        })
    }

    /// Parses a Solidity source file from the given path, generates random values for constants,
    /// and returns the regenerated Solidity source as a string.
    pub fn parse_file(path: &Path) -> anyhow::Result<TemplateFromFile> {
        let source = fs::read_to_string(path)?;
        let mut template =
            TemplateFromFile::parse_str(&source).context("Failed to parse Solidity source")?;
        template.file_path = Some(path.to_path_buf());
        Ok(template)
    }

    /// Randomly selects a template file from the given list and parses it
    pub fn random_file(files: &[PathBuf]) -> anyhow::Result<Self> {
        if files.is_empty() {
            anyhow::bail!("No template files provided for random selection");
        }

        let mut rng = rand::rng();
        let selected_file = files
            .choose(&mut rng)
            .context("Failed to randomly select template file")?;

        Self::parse_file(selected_file)
            .with_context(|| format!("Failed to parse template file: {}", selected_file.display()))
    }

    pub fn set_gen_values(&mut self) -> anyhow::Result<()> {
        for def in self.constants.values_mut() {
            match def.random_variant() {
                Ok(value) => def.generated_value = Some(value),
                Err(e) => {
                    eprintln!("Failed to generate value for constant {}: {}", def.name, e);
                }
            }
        }
        Ok(())
    }

    /// Regenerates the full Solidity source, replacing the RHS of every constant
    /// that has a generated value with its decimal representation.
    /// Constants without a generated value are left unchanged.
    pub fn to_solidity(&self) -> String {
        // Regex that captures the parts of a constant line:
        //   - leading whitespace (indentation)
        //   - constant name
        //   - the original expression (not used, but we need to match up to the semicolon)
        //   - any trailing comment after the semicolon
        let re =
            Regex::new(r"^(\s*uint256\s+\w+\s+constant\s+)(\w+)(\s*=\s*)([^;]+)(;)(.*)$").unwrap();

        let mut new_lines = Vec::new();

        for line in self.original.lines() {
            if let Some(caps) = re.captures(line) {
                let name = &caps[2];
                if let Some(def) = self.constants.get(name)
                    && let Some(value) = &def.generated_value
                {
                    // Reconstruct the line with the new value (decimal representation).
                    let new_line = format!(
                        "{}{}{}{}{}{}",
                        &caps[1], name, &caps[3], value, &caps[5], &caps[6]
                    );
                    new_lines.push(new_line);
                    continue;
                }
            }
            // No match, or constant has no generated value → keep original line.
            new_lines.push(line.to_string());
        }

        new_lines.join("\n")
    }
}
impl TemplateFromFile {
    pub fn template_name(&self) -> String {
        match &self.file_path {
            Some(path) => path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("unknown")
                .to_string(),
            None => "unknown".to_string(),
        }
    }

    pub fn generate_solidity(&self, params: &SolidityFileParams) -> String {
        let template_with_params = Self {
            original: self.original.clone(),
            constants: params.values.clone(),
            file_path: self.file_path.clone(),
        };
        template_with_params.to_solidity()
    }

    pub fn random_params(&self) -> SolidityFileParams {
        let mut mut_params = self.constants.clone();
        for def in mut_params.values_mut() {
            match def.random_variant() {
                Ok(variant) => {
                    def.generated_value = Some(variant);
                }
                Err(_e) => {
                    println!("Failed to generate variant for constant: {}", def.name);
                }
            }
        }
        SolidityFileParams { values: mut_params }
    }
}

/// Definition of a single constant extracted from the Solidity source.
#[derive(Debug, Clone, Hash)]
pub struct ConstantDef {
    pub name: String,
    /// The original right‑hand side expression as a string (e.g., "WAD / 10").
    pub original_expr: String,
    /// An optional randomly generated value (as a U256) that will replace the expression
    /// when regenerating the source.
    pub generated_value: Option<U256>,
    /// An optional range of values to use for generating the constant.
    pub range: Option<StepRange>,
}
impl ConstantDef {
    pub fn parse_str(line: &str) -> anyhow::Result<Self> {
        let re = Regex::new(r"^\s*uint256\s+\w+\s+constant\s+(\w+)\s*=\s*([^;]+);\s*(//.*)?$")?;
        let caps = re
            .captures(line)
            .context("Line does not match constant definition pattern")?;

        let name = caps[1].to_string();
        let expr = caps[2].trim().to_string();
        let comment = caps.get(3).map(|m| m.as_str().trim()).unwrap_or("");

        let range = StepRange::from_comment_str(comment)?;

        Ok(ConstantDef {
            name,
            original_expr: expr,
            generated_value: None,
            range,
        })
    }

    /// Generates a random decimal string within the defined range and step.
    pub fn random_variant(&self) -> anyhow::Result<U256> {
        match &self.range {
            Some(range) => Ok(range.random_value()),
            None => {
                let value =
                    U256::from_dec_str(&self.original_expr).map_err(|e| anyhow::anyhow!(e))?;
                Ok(value)
            }
        }
    }
}
impl std::fmt::Display for ConstantDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(value) = &self.generated_value {
            write!(f, "{}", value)
        } else {
            write!(f, "{}", self.original_expr)
        }
    }
}
impl Serialize for ConstantDef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;

        let mut state = serializer.serialize_struct("ConstantDef", 2)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("original_expr", &self.original_expr)?;

        // Custom serialization for Option<U256>
        match &self.generated_value {
            Some(value) => state.serialize_field("generated_value", &value.to_string())?,
            None => state.serialize_field("generated_value", &Option::<String>::None)?,
        }

        // state.serialize_field("range", &self.range)?;
        state.end()
    }
}
impl<'de> Deserialize<'de> for ConstantDef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct ConstantDefHelper {
            name: String,
            original_expr: String,
            generated_value: Option<String>,
            range: Option<StepRange>,
        }

        let helper = ConstantDefHelper::deserialize(deserializer)?;

        // Convert Option<String> to Option<U256> by parsing the string
        let generated_value = match helper.generated_value {
            Some(s) => Some(U256::from_dec_str(&s).map_err(serde::de::Error::custom)?),
            None => None,
        };

        Ok(ConstantDef {
            name: helper.name,
            original_expr: helper.original_expr,
            generated_value,
            range: helper.range,
        })
    }
}

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct StepRange {
    #[serde(with = "u256_str")]
    pub min: U256,
    #[serde(with = "u256_str")]
    pub max: U256,
    #[serde(with = "u256_str")]
    pub step: U256,
}
impl StepRange {
    pub fn from_comment_str(comment: &str) -> anyhow::Result<Option<Self>> {
        let re =
            Regex::new(r"range\(\s*([0-9.eE+-]+)\s*-\s*([0-9.eE+-]+)\s+by\s+([0-9.eE+-]+)\s*\)")?;
        match re.captures(comment) {
            Some(caps) => {
                let min = Self::parse_u256_scientific(&caps[1])?;
                let max = Self::parse_u256_scientific(&caps[2])?;
                let step = Self::parse_u256_scientific(&caps[3])?;
                if step == U256::zero() {
                    return Err(anyhow::anyhow!("step must be non-zero"));
                }
                Ok(Some(Self { min, max, step }))
            }
            None => Ok(None),
        }
    }

    /// Parses a string that may contain decimal or scientific notation (e.g., "1e16") into a U256.
    fn parse_u256_scientific(s: &str) -> anyhow::Result<U256> {
        let s = s.trim();
        if s.is_empty() {
            return Err(anyhow::anyhow!("empty string"));
        }
        if let Some((mantissa_str, exp_str)) = s.split_once(&['e', 'E'][..]) {
            let mantissa = U256::from_dec_str(mantissa_str.trim())?;
            let exp: i32 = exp_str.trim().parse()?;
            if exp >= 0 {
                // Convert exp to U256 and use pow()
                let factor = U256::from(10).pow(U256::from(exp));
                Ok(mantissa * factor)
            } else {
                // For negative exponent we would divide, but ranges should be positive.
                // Fall back to decimal parsing.
                U256::from_dec_str(s).map_err(|e| anyhow::anyhow!(e))
            }
        } else {
            U256::from_dec_str(s).map_err(|e| anyhow::anyhow!(e))
        }
    }

    pub fn random_value(&self) -> U256 {
        let mut rng = rand::rng();

        if self.step == U256::zero() {
            return self.min;
        }

        // Number of steps from min to max (floor division)
        let range = self.max - self.min;
        let max_steps = range / self.step;
        let total_values = match max_steps.checked_add(U256::one()) {
            Some(val) => val,
            None => U256::MAX,
        };

        // If the range is small enough, use u64 for unbiased sampling.
        let index = if total_values <= U256::from(u64::MAX) {
            let total = total_values.as_u64();
            let idx = rng.random_range(0..total);
            U256::from(idx)
        } else {
            // Generate random U256 by filling bytes
            let mut bytes = [0u8; 32];
            rng.fill(&mut bytes);
            let random_u256 = U256::from_big_endian(&bytes);
            random_u256 % total_values
        };

        self.min + index * self.step
    }

    pub fn variation(&self, current: U256) -> U256 {
        let mut rng = rand::rng();

        if current < self.min || current > self.max {
            return current;
        }

        let delta = random_u256(&mut rng) % (self.step.saturating_add(U256::from(1)));
        let multiple_delta = delta * 10;
        if rng.random_bool(0.5) {
            current.saturating_add(multiple_delta)
        } else {
            current.saturating_sub(multiple_delta)
        }
        .clamp(self.min, self.max)
    }
}

/// Strategy parameters: a map from constant name to its numerical value.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SolidityFileParams {
    pub values: HashMap<String, ConstantDef>,
}
impl SolidityFileParams {
    pub(crate) fn vary_one(
        &self,
        constant_defs: &HashMap<String, ConstantDef>,
    ) -> anyhow::Result<SolidityFileParams> {
        if self.values.is_empty() {
            return Err(anyhow::anyhow!("No values to vary"));
        }
        // Pick a random key
        let mut rng = rand::rng();
        let keys: Vec<&String> = self.values.keys().collect();
        let picked_key = keys[rng.random_range(0..keys.len())];

        let mut new_params = self.clone();

        if let Some(mut_constant_def) = new_params.values.get_mut(picked_key) {
            let current_value = match mut_constant_def.generated_value {
                Some(val) => val,
                None => match &mut_constant_def.range {
                    Some(range) => range.random_value(),
                    None => todo!(),
                },
            };

            let new_value = match &mut_constant_def.range {
                Some(range) => range.variation(current_value),
                None => {
                    let defs = constant_defs.get(picked_key).ok_or_else(|| {
                        anyhow::anyhow!("No constant def for key: {}", picked_key)
                    })?;
                    let range = defs.range.as_ref().ok_or_else(|| {
                        anyhow::anyhow!("No range for constant def: {}", picked_key)
                    })?;
                    range.variation(current_value)
                }
            };

            println!(
                "changed {}: current_value={} new_value={}",
                picked_key, current_value, new_value
            );

            // Store the new value as a decimal string
            mut_constant_def.generated_value = Some(new_value);
        }

        Ok(new_params)
    }
}
impl SolidityFileParams {
    /// Returns a string representation of the parameters (e.g., for logging).
    pub fn id(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        let mut entries: Vec<_> = self.values.iter().collect();
        entries.sort_by_key(|(name, _)| *name);
        for (name, val) in entries {
            name.hash(&mut hasher);
            val.hash(&mut hasher);
        }
        format!("{:x}", hasher.finish())
    }

    /// Returns a short suffix that can be used to name a strategy instance.
    pub fn name_suffix(&self) -> String {
        // Use the first few characters of the first value's hex representation.
        if let Some((_, val)) = self.values.iter().next() {
            if let Some(generated_value) = &val.generated_value {
                format!("_{}", generated_value).chars().take(8).collect()
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    }
}
impl std::fmt::Display for SolidityFileParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "TemplateFromFile:")?;
        for (name, val) in &self.values {
            let value_str = match &val.generated_value {
                Some(v) => v.to_string(),
                None => "none".to_string(),
            };
            writeln!(f, "  {}: {}", name, value_str)?;
        }
        Ok(())
    }
}

fn random_u256(rng: &mut ThreadRng) -> U256 {
    let mut bytes = [0u8; 32]; // 256 bits = 32 bytes
    rng.fill(&mut bytes[..]);
    U256::from_little_endian(&bytes)
}

mod u256_str {
    use primitive_types::U256;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &U256, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<U256, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        U256::from_dec_str(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::REPO;
    use std::path::PathBuf;

    #[test]
    fn test_parse_file() -> anyhow::Result<()> {
        let file_path: PathBuf = REPO.join("contracts/src/Variance.sol");
        let template = TemplateFromFile::parse_file(&file_path)?;
        assert_eq!(template.constants.len(), 3);
        assert!(template.constants.contains_key("ALPHA"));
        assert!(template.constants.contains_key("FEE_SCALE"));
        assert!(template.constants.contains_key("BASE_FEE"));
        Ok(())
    }

    #[test]
    fn test_parse_regenerate() -> anyhow::Result<()> {
        const SAMPLE_SOURCE: &str = r#"
            /// @notice Smoothing factor for EWMA (10% = 1e17). Must be in WAD.
            uint256 internal constant ALPHA = WAD / 10; // 0.1  range(1e16-1e17 by 1e16)

            /// @notice Scaling factor from volatility (WAD) to fee increment (WAD)
            /// @dev e.g., 10% volatility adds 1% fee: (0.1 * 0.1) = 0.01 = 1e16
            uint256 internal constant FEE_SCALE = 1e16; // 1% in WAD

            /// @notice Base fee applied when volatility is zero (5 bps = 5e14)
            uint256 internal constant BASE_FEE = 5 * BPS;
        "#;

        let mut template = TemplateFromFile::parse_str(SAMPLE_SOURCE).unwrap();

        assert_eq!(template.constants.len(), 3);
        assert!(template.constants.contains_key("ALPHA"));
        assert_eq!(template.constants["ALPHA"].original_expr, "WAD / 10");
        assert_eq!(template.constants["FEE_SCALE"].original_expr, "1e16");
        assert_eq!(template.constants["BASE_FEE"].original_expr, "5 * BPS");
        // let mut rng = StdRng::seed_from_u64(42);
        template.set_gen_values()?;

        // Check that values were assigned
        assert!(template.constants["ALPHA"].generated_value.is_some()); // only alpha has a range to generate values

        let regenerated = template.to_solidity();
        println!("{}", regenerated);

        // The regenerated source should still contain the same line structure
        // and comments, but with decimal numbers instead of original expressions.
        for line in regenerated.lines() {
            if line.contains("ALPHA") {
                assert!(line.contains("= "));
                assert!(!line.contains("WAD / 10")); // range specified -> expr replaced by number
                assert!(line.contains("; // 0.1"));
            }
            if line.contains("FEE_SCALE") {
                assert!(line.contains("1e16")); // no range specified -> expr unchanged
                assert!(line.contains("; // 1% in WAD"));
            }
            if line.contains("BASE_FEE") {
                assert!(line.contains("5 * BPS")); // no range specified -> expr unchanged
                assert!(line.contains(";"));
            }
        }

        // Verify that we can parse the regenerated source again (it should still be valid Solidity).
        let reparsed = TemplateFromFile::parse_str(&regenerated).unwrap();
        assert_eq!(reparsed.constants.len(), 3);
        // The regenerated constants now have the new values as expressions.
        for (name, reparsed_def) in &reparsed.constants {
            let original_def = &template.constants[name];
            if let Some(expected_value) = original_def.generated_value {
                assert_eq!(reparsed_def.original_expr, expected_value.to_string());
            }
        }
        Ok(())
    }

    #[test]
    fn test_parse() -> anyhow::Result<()> {
        const SOLIDITY: &str = r#"
          /// @notice Smoothing factor for EWMA (0.9 = 90%). Must be in WAD.
          /// @dev Higher lambda = more weight on recent values
          uint256 public constant LAMBDA_WAD = 9e17; // range(1e17-1e18 by 1e17)

          /// @notice Base fee applied to all transactions (14 bps = 0.0014%)
          /// @dev In WAD units for fee calculations
          uint256 public constant BASE_FEE_WAD = 14 * BPS; // range(1e12-4e12 by 1e12)

          /// @notice Sensitivity multiplier for volatility/sigma (2.0x). Must be in WAD.
          /// @dev Scales raw volatility to fee adjustment magnitude
          uint256 public constant SENSITIVITY_WAD = 2e18; // range(1e18-5e18 by 1e17)
        "#;
        let parsed = TemplateFromFile::parse_str(SOLIDITY).unwrap();
        assert_eq!(parsed.constants.len(), 3);

        Ok(())
    }

    #[test]
    fn test_serialize_full_constant_def() {
        let constant = ConstantDef {
            name: "WAD".to_string(),
            original_expr: "10 ** 18".to_string(),
            generated_value: Some(U256::from(1000000000)),
            range: Some(StepRange {
                min: U256::from(1),
                max: U256::from(100),
                step: U256::from(1),
            }),
        };

        let json = serde_json::to_string(&constant).unwrap();

        let expected =
            r#"{"name":"WAD","original_expr":"10 ** 18","generated_value":"1000000000"}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn steprange_variation_returns_different_numbers() {
        // Create a range with a reasonable step
        let range = StepRange {
            min: U256::from(100u64),
            max: U256::from(200u64),
            step: U256::from(10u64),
        };

        // Pick a current value inside the range
        let current = U256::from(150u64);

        // Collect many variation results
        let iterations = 1000;
        let mut results = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            results.push(range.variation(current));
        }

        // Ensure at least two distinct values appear
        let first = results[0];
        let has_variation = results.iter().any(|&v| v != first);
        assert!(
            has_variation,
            "All {} variation calls returned the same value: {}",
            iterations, first
        );

        // Optional: verify all results stay within the range bounds
        for &value in &results {
            assert!(
                value >= range.min && value <= range.max,
                "Value {} is out of range [{}, {}]",
                value,
                range.min,
                range.max
            );
        }
    }

    #[test]
    fn steprange_variation_works_at_boundaries() {
        let range = StepRange {
            min: U256::from(0u64),
            max: U256::from(100u64),
            step: U256::from(25u64),
        };

        // At min, variation should not go below min
        let at_min = range.variation(range.min);
        assert!(at_min >= range.min);

        // At max, variation should not exceed max
        let at_max = range.variation(range.max);
        assert!(at_max <= range.max);
    }
}
