//! Derive a linker symbol order from executed code, so the functions a run
//! executes share pages instead of each faulting in its own neighbourhood.
//!
//! Inputs are cachegrind profiles (`--cache-sim=no --demangle=no`) of the
//! binary the order is for: symbol names carry the build's crate hashes, so
//! train and link the same profile. Names survive relinking, and cachegrind
//! keeps no call stack, which the lane's sibling-call chains would exhaust.
//! Functions are listed in first-appearance order across profiles, so a run
//! of the first profile's kind touches one contiguous region. (Ordering hot
//! functions first within a profile's group measured more resident pages.)
//! Layout changes, behavior does not.

use std::{collections::HashSet, env, fs, process::Command, process::ExitCode};

/// The suffix thin LTO gives a promoted local symbol: a module hash, which
/// changes between builds while the function is the same.
const LTO_LOCAL_SUFFIX: &str = ".llvm.";

/// A symbol's name without its LTO module hash.
fn stable_name(name: &str) -> &str {
    name.split_once(LTO_LOCAL_SUFFIX)
        .map_or(name, |(stable, _)| stable)
}

/// `nm` symbol kinds that name code.
const CODE_KINDS: &[&str] = &["t", "T", "w", "W"];

/// The binary's code symbols, by mangled name.
fn code_symbols(binary: &str) -> Result<Vec<String>, String> {
    let output = Command::new("nm")
        .args(["--defined-only", binary])
        .output()
        .map_err(|error| format!("nm: {error}"))?;
    let text = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
    Ok(text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (_, kind, name) = (fields.next()?, fields.next()?, fields.next()?);
            CODE_KINDS.contains(&kind).then(|| name.to_owned())
        })
        .collect())
}

/// The functions a profile executed, in first-appearance order.
fn executed(profile: &str) -> Result<Vec<String>, String> {
    let text = fs::read_to_string(profile).map_err(|error| format!("{profile}: {error}"))?;
    Ok(text
        .lines()
        .filter_map(|line| line.strip_prefix("fn=").map(str::to_owned))
        .collect())
}

fn run(binary: &str, out: &str, profiles: &[String]) -> Result<(), String> {
    let symbols = code_symbols(binary)?;
    // Profiles may come from an earlier link of the same build: match names
    // without LTO module hashes, and write the binary's current names.
    let known: std::collections::HashMap<&str, &str> = symbols
        .iter()
        .map(|name| (stable_name(name), name.as_str()))
        .collect();
    let mut seen = HashSet::new();
    let mut order: Vec<&str> = Vec::new();
    for profile in profiles {
        for name in executed(profile)? {
            if let Some(name) = known.get(stable_name(&name)).copied() {
                if seen.insert(name) {
                    order.push(name);
                }
            }
        }
    }
    let mut text = order.join("\n");
    text.push('\n');
    fs::write(out, text).map_err(|error| format!("{out}: {error}"))?;
    eprintln!("{} symbols", order.len());
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let [binary, out, profiles @ ..] = args.as_slice() else {
        eprintln!("usage: symbol-order <binary> <out.order> <cachegrind.out>...");
        return ExitCode::from(2);
    };
    match run(binary, out, profiles) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}
