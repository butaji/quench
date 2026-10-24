//! Derive `llvm-bolt` inputs from executed instructions, so the basic blocks a run executes
//! share pages instead of sitting among the cold blocks of their functions.
//!
//! Inputs are callgrind profiles (`--dump-instr=yes --dump-line=no --demangle=no`) and the
//! binary they ran on, linked with `--emit-relocs`; its symbols place each address. Counts are
//! written per function offset, so they apply to a relink of the same build with another
//! symbol order, the binary BOLT rewrites. Every
//! executed instruction's count becomes a basic sample (BOLT's no-LBR profile), which gives
//! each block its execution count; BOLT moves the executed blocks of each function into hot
//! text, in `--order`'s function order, and the never-executed ones into cold text.
//!
//! Outputs, in the output directory:
//! - `profile.fdata`: the samples;
//! - `function.order`: `--order` (a `symbol-order` file) in BOLT's function names;
//! - `skip.funcs`: the functions BOLT leaves in place, all code not generated from Rust.
//!   glibc's single-thread atomics jump over a `lock` prefix, which BOLT splits off its
//!   instruction; the linked order keeps that code together instead (`symbol-order
//!   --foreign-first`).
//!
//! `--dense PATTERN` counts every instruction of the functions whose symbol contains
//! PATTERN as executed, so they stay whole in hot text: the interpreter's dispatch
//! (`run_frame_general_until`, the lane handlers `13dispatch_fast`) reaches whichever
//! blocks a program's code needs, which short training programs rarely cover.

use std::{
    collections::{BTreeMap, HashMap},
    env, fs,
    io::{BufRead, BufReader, Write},
    process::{Command, ExitCode, Stdio},
};

/// The mangling prefix of Rust symbols (v0).
const RUST_SYMBOL_PREFIX: &str = "_R";

/// `nm` symbol kinds that name code.
const CODE_KINDS: &[&str] = &["t", "T", "w", "W"];

/// The `nm` kind of local code, whose BOLT name carries a disambiguating count.
const LOCAL_CODE_KIND: &str = "t";

/// The header of a basic-sample (no-LBR) BOLT profile.
const NO_LBR_HEADER: &str = "no_lbr";

/// A code symbol of the binary.
struct Symbol {
    address: u64,
    size: u64,
    /// The mangled name, as `symbol-order` writes it.
    name: String,
    /// BOLT's name: a local function is `name/k`, the k-th local symbol of that name in
    /// symbol-table order.
    bolt_name: String,
}

/// The binary's code symbols in symbol-table order (`nm -p`), with BOLT's names.
fn code_symbols(binary: &str) -> Result<Vec<Symbol>, String> {
    let output = Command::new("nm")
        .args(["-p", "-S", "--defined-only", binary])
        .output()
        .map_err(|error| format!("nm: {error}"))?;
    let text = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
    let mut locals: HashMap<String, usize> = HashMap::new();
    let mut symbols = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [address, size, kind, name] = fields[..] else {
            continue;
        };
        if !CODE_KINDS.contains(&kind) {
            continue;
        }
        let bolt_name = if kind == LOCAL_CODE_KIND {
            let count = locals.entry(name.to_owned()).or_default();
            *count += 1;
            format!("{name}/{count}")
        } else {
            name.to_owned()
        };
        let (Ok(address), Ok(size)) = (
            u64::from_str_radix(address, 16),
            u64::from_str_radix(size, 16),
        ) else {
            continue;
        };
        symbols.push(Symbol {
            address,
            size,
            name: name.to_owned(),
            bolt_name,
        });
    }
    Ok(symbols)
}

/// Adds each executed instruction's count from a callgrind profile. A position is absolute
/// (`0x...`), relative to the previous one (`+n`, `-n`) or the same (`*`); the cost line after
/// `calls=` is a call's inclusive cost, not its instruction's.
fn add_counts(profile: &str, counts: &mut HashMap<u64, u64>) -> Result<(), String> {
    let file = fs::File::open(profile).map_err(|error| format!("{profile}: {error}"))?;
    let mut address = 0u64;
    let mut call_cost = false;
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|error| format!("{profile}: {error}"))?;
        if line.starts_with("calls=") {
            call_cost = true;
            continue;
        }
        let mut fields = line.split_whitespace();
        let Some(position) = fields.next() else {
            continue;
        };
        address = if let Some(hex) = position.strip_prefix("0x") {
            u64::from_str_radix(hex, 16).map_err(|error| format!("{profile}: {error}"))?
        } else if let Some(delta) = position.strip_prefix('+') {
            address + delta.parse::<u64>().map_err(|error| error.to_string())?
        } else if let Some(delta) = position.strip_prefix('-') {
            address - delta.parse::<u64>().map_err(|error| error.to_string())?
        } else if position == "*" {
            address
        } else {
            continue;
        };
        if std::mem::take(&mut call_cost) {
            continue;
        }
        if let Some(count) = fields.next().and_then(|count| count.parse::<u64>().ok()) {
            *counts.entry(address).or_default() += count;
        }
    }
    Ok(())
}

/// Calls `visit` with every instruction address `objdump` disassembles in the binary.
fn instructions(binary: &str, mut visit: impl FnMut(u64)) -> Result<(), String> {
    let mut child = Command::new("objdump")
        .args(["-d", "--no-show-raw-insn", binary])
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("objdump: {error}"))?;
    let stdout = child.stdout.take().ok_or("objdump: no output")?;
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|error| format!("objdump: {error}"))?;
        let address = line.trim_start().split_once(":\t");
        if let Some(Ok(address)) = address.map(|(address, _)| u64::from_str_radix(address, 16)) {
            visit(address);
        }
    }
    child.wait().map_err(|error| format!("objdump: {error}"))?;
    Ok(())
}

fn run(
    binary: &str,
    out: &str,
    order: Option<&str>,
    dense: &[String],
    profiles: &[String],
) -> Result<(), String> {
    let symbols = code_symbols(binary)?;
    let mut counts = HashMap::new();
    for profile in profiles {
        add_counts(profile, &mut counts)?;
    }
    // Aliases share an address; BOLT knows the function by any of its names.
    let mut by_address: BTreeMap<u64, usize> = BTreeMap::new();
    for (index, symbol) in symbols.iter().enumerate() {
        if symbol.size > 0 {
            by_address.entry(symbol.address).or_insert(index);
        }
    }
    let containing = |address: u64| {
        let (_, &index) = by_address.range(..=address).next_back()?;
        let symbol = &symbols[index];
        (address < symbol.address + symbol.size).then_some(index)
    };
    let mut samples: BTreeMap<(usize, u64), u64> = BTreeMap::new();
    for (&address, &count) in &counts {
        if let Some(index) = containing(address) {
            *samples
                .entry((index, address - symbols[index].address))
                .or_default() += count;
        }
    }
    if !dense.is_empty() {
        let is_dense: Vec<bool> = symbols
            .iter()
            .map(|symbol| {
                dense
                    .iter()
                    .any(|pattern| symbol.name.contains(pattern.as_str()))
            })
            .collect();
        instructions(binary, |address| {
            if let Some(index) = containing(address).filter(|&index| is_dense[index]) {
                samples
                    .entry((index, address - symbols[index].address))
                    .or_insert(1);
            }
        })?;
    }
    fs::create_dir_all(out).map_err(|error| format!("{out}: {error}"))?;
    let mut fdata = Vec::new();
    writeln!(fdata, "{NO_LBR_HEADER}").map_err(|error| error.to_string())?;
    for (&(index, offset), count) in &samples {
        writeln!(fdata, "1 {} {offset:x} {count}", symbols[index].bolt_name)
            .map_err(|error| error.to_string())?;
    }
    fs::write(format!("{out}/profile.fdata"), fdata).map_err(|error| error.to_string())?;
    let skip: Vec<&str> = symbols
        .iter()
        .filter(|symbol| !symbol.name.starts_with(RUST_SYMBOL_PREFIX))
        .map(|symbol| symbol.bolt_name.as_str())
        .collect();
    fs::write(format!("{out}/skip.funcs"), skip.join("\n") + "\n")
        .map_err(|error| error.to_string())?;
    if let Some(order) = order {
        let text = fs::read_to_string(order).map_err(|error| format!("{order}: {error}"))?;
        let mut names: HashMap<&str, Vec<&str>> = HashMap::new();
        for symbol in &symbols {
            names
                .entry(symbol.name.as_str())
                .or_default()
                .push(symbol.bolt_name.as_str());
        }
        let ordered: Vec<&str> = text
            .lines()
            .flat_map(|name| names.get(name.trim()).into_iter().flatten().copied())
            .collect();
        fs::write(format!("{out}/function.order"), ordered.join("\n") + "\n")
            .map_err(|error| error.to_string())?;
    }
    eprintln!("{} samples", samples.len());
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let [binary, out, rest @ ..] = args.as_slice() else {
        eprintln!("usage: bolt-profile <binary> <out-dir> [--order FILE] [--dense PATTERN]... <callgrind.out>...");
        return ExitCode::from(2);
    };
    let mut order = None;
    let mut dense = Vec::new();
    let mut profiles = Vec::new();
    let mut arguments = rest.iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--order" => order = arguments.next().cloned(),
            "--dense" => dense.extend(arguments.next().cloned()),
            _ => profiles.push(argument.clone()),
        }
    }
    match run(binary, out, order.as_deref(), &dense, &profiles) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}
