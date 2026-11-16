//! Step 3: the JIT completeness census may not move away from done (`quench-lint --census`),
//! and no tier may write an opcode's meaning by hand that `staged/hand_semantics.txt` does not
//! allow (the runtime test that reads the table).

use crate::{config::Config, lint::LINT, process, runtime_tests, step::Verdict};

const LINT_CENSUS: &str = "--census";
/// The runtime test that holds the tiers to the interpreter's opcode meanings.
const HAND_SEMANTICS: &str = "tiers_derive_opcode_meaning_from_the_interpreter";

pub fn run(config: &Config) -> Verdict {
    let lint = config.binary(LINT)?;
    let census = process::run(config.command(lint).arg(LINT_CENSUS))?;
    if !census.success {
        return Err(census.excerpt());
    }
    let tests = runtime_tests::run(config, &[HAND_SEMANTICS.to_owned()])?;
    Ok(format!("census holds, {tests} hand-semantics test passed"))
}
