//! Derive a linker symbol order from executed code, so the functions a run
//! executes share pages instead of each faulting in its own neighbourhood.
//!
//! Inputs are valgrind profiles of the binary the order is for (symbol names
//! carry the build's crate hashes, so train and link the same profile):
//! callgrind with `--dump-instr=yes --compress-pos=no` names executed
//! instruction addresses, and cachegrind or callgrind with `--demangle=no`
//! name executed functions (`fn=` records). Cachegrind keeps no call stack,
//! so it profiles long sibling-call chains that exhaust callgrind's.
//! Functions are listed in first-appearance order across profiles; layout
//! changes, behavior does not.

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

/// What a profile record says executed.
enum Executed {
    Address(u64),
    Function(String),
}

/// The executed addresses and function names of a profile, in file order.
fn executed(profile: &str) -> Result<Vec<Executed>, String> {
    let text = fs::read_to_string(profile).map_err(|error| format!("{profile}: {error}"))?;
    Ok(text
        .lines()
        .filter_map(|line| {
            if let Some(function) = line.strip_prefix("fn=") {
                // Callgrind compresses repeated names to `(id)`; the first
                // record of an id carries the name after it.
                let name = match function.strip_prefix('(') {
                    Some(rest) => rest.split_once(") ")?.1,
                    None => function,
                };
                return Some(Executed::Function(name.to_owned()));
            }
            let address = line.split_whitespace().next()?.strip_prefix("0x")?;
            u64::from_str_radix(address, 16).ok().map(Executed::Address)
        })
        .collect())
}

fn run(binary: &str, out: &str, profiles: &[String]) -> Result<(), String> {
    let symbols = symbols(binary)?;
    let mut seen = HashSet::new();
    let mut order = Vec::new();
    let names: HashSet<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
    for profile in profiles {
        for record in executed(profile)? {
            let name = match record {
                Executed::Address(address) => {
                    let index = symbols.partition_point(|symbol| symbol.start <= address);
                    let Some(symbol) = index.checked_sub(1).map(|index| &symbols[index]) else {
                        continue;
                    };
                    if address >= symbol.start + symbol.size {
                        continue;
                    }
                    symbol.name.as_str()
                }
                Executed::Function(function) => match names.get(function.as_str()) {
                    Some(name) => name,
                    None => continue,
                },
            };
            if seen.insert(name) {
                order.push(name);
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
