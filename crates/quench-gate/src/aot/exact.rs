//! The generic-row budget (`crates/quench-runtime/aot-exact-steps.txt`): per benchmark source,
//! the records that no ahead lane body lowers, so AOT trampolines hand them to the per-PC
//! fallback. It must be all zero under `--strict`; its total is reported otherwise.

use crate::config::Config;

const BUDGET: &str = "crates/quench-runtime/aot-exact-steps.txt";
const COMMENT: char = '#';

/// The total of the budget's counts: each data line starts with its count.
fn total(text: &str) -> Result<u64, String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(COMMENT))
        .map(|line| {
            let count = line.split_whitespace().next().unwrap_or_default();
            count.parse::<u64>().map_err(|_| format!("`{line}`: no count"))
        })
        .sum()
}

/// The budget's report line, or its failure when `--strict` finds records left.
pub fn check(config: &Config) -> Result<String, String> {
    let path = config.root.join(BUDGET);
    let text = std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let records = total(&text)?;
    if config.strict && records != 0 {
        return Err(format!("{records} generic-row records have no ahead lane body (--strict requires 0)"));
    }
    Ok(format!("generic-row budget: {records} records"))
}
