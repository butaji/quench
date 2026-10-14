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
//!
//! Profiles after `--run` describe a run phase instead: callgrind dumps split
//! at a start-of-run marker (`--dump-before`), whose final part holds only what
//! executes once start-up is over. Those functions come first, the first run
//! profile's in its order and the rest by how many run profiles executed them,
//! so the code every run touches is dense and start-up code stays apart.

use std::{collections::HashSet, env, fs, process::Command, process::ExitCode};

/// The suffix thin LTO gives a promoted local symbol: a module hash, which
/// changes between builds while the function is the same.
const LTO_LOCAL_SUFFIX: &str = ".llvm.";

/// A symbol's name without its LTO module hash.
fn stable_name(name: &str) -> &str {
    name.split_once(LTO_LOCAL_SUFFIX)
        .map_or(name, |(stable, _)| stable)
}

/// The C runtime's entry and exit code: every run executes it, and valgrind
/// attributes it to no function, so it leads the order explicitly.
const PROCESS_RUNTIME: &[&str] = &[
    "_start",
    "deregister_tm_clones",
    "register_tm_clones",
    "__do_global_dtors_aux",
    "frame_dummy",
];

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

/// The functions a profile executed, in first-appearance order. Callgrind
/// compresses a repeated name to its `(id)`, which the first use defines.
fn executed(profile: &str) -> Result<Vec<String>, String> {
    let text = fs::read_to_string(profile).map_err(|error| format!("{profile}: {error}"))?;
    let mut compressed = std::collections::HashMap::new();
    let mut seen = HashSet::new();
    let mut functions = Vec::new();
    for line in text.lines() {
        // A callee line (`cfn=`) may define a compressed name first.
        if let Some((id, defined)) = line
            .strip_prefix("cfn=(")
            .and_then(|rest| rest.split_once(')'))
            .filter(|(_, defined)| !defined.is_empty())
        {
            compressed.insert(id.to_owned(), defined.trim_start().to_owned());
            continue;
        }
        let Some(name) = line.strip_prefix("fn=") else {
            continue;
        };
        let name = match name.strip_prefix('(').and_then(|rest| rest.split_once(')')) {
            Some((id, "")) => compressed.get(id).cloned(),
            Some((id, defined)) => {
                let defined = defined.trim_start().to_owned();
                compressed.insert(id.to_owned(), defined.clone());
                Some(defined)
            }
            None => Some(name.to_owned()),
        };
        if let Some(name) = name.filter(|name| seen.insert(name.clone())) {
            functions.push(name);
        }
    }
    Ok(functions)
}

/// The run profiles' functions: the first profile's in its order, then the
/// others' by how many profiles executed them, ties in first-appearance order.
fn run_phase(profiles: &[String]) -> Result<Vec<String>, String> {
    let Some((first, rest)) = profiles.split_first() else {
        return Ok(Vec::new());
    };
    let mut order = executed(first)?;
    let mut counts: std::collections::HashMap<String, (usize, usize)> = std::collections::HashMap::new();
    for profile in rest {
        for name in executed(profile)? {
            let next = counts.len();
            counts.entry(stable_name(&name).to_owned()).or_insert((0, next)).0 += 1;
        }
    }
    let mut ranked: Vec<_> = counts.into_iter().collect();
    ranked.sort_by_key(|(_, (count, first_seen))| (std::cmp::Reverse(*count), *first_seen));
    order.extend(ranked.into_iter().map(|(name, _)| name));
    Ok(order)
}

fn run(binary: &str, out: &str, profiles: &[String], run: &[String]) -> Result<(), String> {
    let symbols = code_symbols(binary)?;
    // Profiles may come from an earlier link of the same build: match names
    // without LTO module hashes, and write the binary's current names. A
    // name can belong to several local copies, which a profile cannot tell
    // apart; every copy is ordered.
    let mut known: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
    for name in &symbols {
        known.entry(stable_name(name)).or_default().push(name);
    }
    let mut seen = HashSet::new();
    let mut order: Vec<&str> = Vec::new();
    for name in run_phase(run)? {
        for name in known.get(stable_name(&name)).into_iter().flatten() {
            if seen.insert(*name) {
                order.push(name);
            }
        }
    }
    let runtime = PROCESS_RUNTIME.iter().map(|name| (*name).to_owned());
    for profile in profiles {
        for name in runtime.clone().chain(executed(profile)?) {
            for name in known.get(stable_name(&name)).into_iter().flatten() {
                if seen.insert(*name) {
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
        eprintln!("usage: symbol-order <binary> <out.order> <cachegrind.out>... [--run <callgrind.out>...]");
        return ExitCode::from(2);
    };
    let (profiles, run_profiles) = match profiles.iter().position(|arg| arg == "--run") {
        Some(index) => (&profiles[..index], &profiles[index + 1..]),
        None => (profiles, &[][..]),
    };
    match run(binary, out, profiles, run_profiles) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}
