//! Derive a linker symbol order from executed code, so the functions a run
//! executes share pages instead of each faulting in its own neighbourhood.
//!
//! Inputs are callgrind profiles recorded with `--dump-instr=yes
//! --compress-pos=no` of the binary the order is for (symbol names carry the
//! build's crate hashes, so train and link the same profile). Functions are
//! listed in first-execution order across profiles; layout changes, behavior
//! does not.

use std::{collections::HashSet, env, fs, process::Command, process::ExitCode};

/// A function symbol: start address, size and mangled name.
struct Symbol {
    start: u64,
    size: u64,
    name: String,
}

/// `nm` symbol kinds that name code.
const CODE_KINDS: &[&str] = &["t", "T", "w", "W"];

fn symbols(binary: &str) -> Result<Vec<Symbol>, String> {
    let output = Command::new("nm")
        .args(["--defined-only", "-S", binary])
        .output()
        .map_err(|error| format!("nm: {error}"))?;
    let text = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
    let mut symbols: Vec<Symbol> = text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (start, size, kind, name) = (
                fields.next()?,
                fields.next()?,
                fields.next()?,
                fields.next()?,
            );
            CODE_KINDS.contains(&kind).then_some(())?;
            Some(Symbol {
                start: u64::from_str_radix(start, 16).ok()?,
                size: u64::from_str_radix(size, 16).ok()?,
                name: name.to_owned(),
            })
        })
        .collect();
    symbols.sort_by_key(|symbol| symbol.start);
    Ok(symbols)
}

/// Instruction addresses of a callgrind profile, in file order.
fn executed(profile: &str) -> Result<impl Iterator<Item = u64>, String> {
    let text = fs::read_to_string(profile).map_err(|error| format!("{profile}: {error}"))?;
    let addresses: Vec<u64> = text
        .lines()
        .filter_map(|line| {
            let address = line.split_whitespace().next()?.strip_prefix("0x")?;
            u64::from_str_radix(address, 16).ok()
        })
        .collect();
    Ok(addresses.into_iter())
}

fn run(binary: &str, out: &str, profiles: &[String]) -> Result<(), String> {
    let symbols = symbols(binary)?;
    let mut seen = HashSet::new();
    let mut order = Vec::new();
    for profile in profiles {
        for address in executed(profile)? {
            let index = symbols.partition_point(|symbol| symbol.start <= address);
            let Some(symbol) = index.checked_sub(1).map(|index| &symbols[index]) else {
                continue;
            };
            if address < symbol.start + symbol.size && seen.insert(symbol.name.as_str()) {
                order.push(symbol.name.as_str());
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
        eprintln!("usage: symbol-order <binary> <out.order> <callgrind.out>...");
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
