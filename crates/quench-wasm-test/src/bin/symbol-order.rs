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
//! Each further `--run` group is a later tier, ranked on its own after the
//! first and the dense family. `--dense PATTERN` places every function whose symbol contains PATTERN
//! (the interpreter's per-instruction lane handlers, `13dispatch_fast`): a run
//! reaches whichever handlers its code needs, so the whole family stays packed.
//! `--foreign-first` moves the code not generated from Rust ahead, keeping its order: in a
//! link that `llvm-bolt` rewrites, BOLT moves the Rust functions into its own hot text and
//! leaves that code in place (`bolt-profile`), packed instead of scattered through the gaps.

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

/// The mangling prefix of Rust symbols (v0).
const RUST_SYMBOL_PREFIX: &str = "_R";

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

/// Functions by how many of `profiles` executed them, ties in first-appearance order.
fn ranked(profiles: &[String]) -> Result<Vec<String>, String> {
    let mut counts: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new();
    for profile in profiles {
        for name in executed(profile)? {
            let next = counts.len();
            counts
                .entry(stable_name(&name).to_owned())
                .or_insert((0, next))
                .0 += 1;
        }
    }
    let mut ranked: Vec<_> = counts.into_iter().collect();
    ranked.sort_by_key(|(_, (count, first_seen))| (std::cmp::Reverse(*count), *first_seen));
    Ok(ranked.into_iter().map(|(name, _)| name).collect())
}

/// The first run tier's functions: its first profile's in order, then the others' ranked.
fn run_phase(profiles: &[String]) -> Result<Vec<String>, String> {
    let Some((first, rest)) = profiles.split_first() else {
        return Ok(Vec::new());
    };
    let mut order = executed(first)?;
    order.extend(ranked(rest)?);
    Ok(order)
}

/// Appends the binary's copies of `names` not yet ordered.
fn place<'a>(
    names: impl IntoIterator<Item = String>,
    known: &std::collections::HashMap<&'a str, Vec<&'a str>>,
    seen: &mut HashSet<&'a str>,
    order: &mut Vec<&'a str>,
) {
    for name in names {
        for name in known.get(stable_name(&name)).into_iter().flatten() {
            if seen.insert(*name) {
                order.push(name);
            }
        }
    }
}

fn run(
    binary: &str,
    out: &str,
    profiles: &[String],
    run: &[Vec<String>],
    dense: &[String],
    foreign_first: bool,
) -> Result<(), String> {
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
    let (first_tier, later_tiers) = run
        .split_first()
        .map_or((&[][..], &[][..]), |(first, later)| {
            (first.as_slice(), later)
        });
    place(run_phase(first_tier)?, &known, &mut seen, &mut order);
    let dense_names = symbols
        .iter()
        .filter(|name| dense.iter().any(|pattern| name.contains(pattern.as_str())))
        .cloned();
    place(dense_names, &known, &mut seen, &mut order);
    for tier in later_tiers {
        place(ranked(tier)?, &known, &mut seen, &mut order);
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
    if foreign_first {
        order.sort_by_key(|name| name.starts_with(RUST_SYMBOL_PREFIX));
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
        eprintln!("usage: symbol-order <binary> <out.order> [--foreign-first] [--dense PATTERN]... <cachegrind.out>... [--run <callgrind.out>...]...");
        return ExitCode::from(2);
    };
    let mut dense = Vec::new();
    let mut foreign_first = false;
    let mut rest = Vec::new();
    let mut arguments = profiles.iter();
    while let Some(argument) = arguments.next() {
        match (argument.as_str(), arguments.clone().next()) {
            ("--foreign-first", _) => foreign_first = true,
            ("--dense", Some(pattern)) => {
                dense.push(pattern.clone());
                arguments.next();
            }
            _ => rest.push(argument.clone()),
        }
    }
    // Each `--run` starts a run tier; profiles before the first are start-up profiles.
    let mut groups = rest.split(|arg| arg == "--run");
    let profiles = groups.next().unwrap_or_default().to_vec();
    let run_profiles: Vec<Vec<String>> = groups.map(<[String]>::to_vec).collect();
    match run(binary, out, &profiles, &run_profiles, &dense, foreign_first) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}
