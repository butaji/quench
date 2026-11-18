//! Step: the C twins' instruction counts, the denominator of the kernel C-parity ratios.
//!
//! `c-parity` records what a kernel's hottest loop spends per iteration on every Quench tier
//! (`cparity*.txt`); this step records what the same kernel's C twin spends, measured the same
//! way from machine code: each `cparity/c/*.c` is compiled with `clang -O2 -mcpu=apple-m4`,
//! disassembled by the host's `objdump`, and its work loop — the largest innermost loop of the
//! object's text — is counted per iteration. A second build adds `-fno-vectorize
//! -fno-unroll-loops`, the convention `ir_budget.txt` states for its C column, so a scalar Quench
//! loop is compared with a scalar C loop rather than with eight lanes of a vectorized one.
//!
//! The counts are a ratchet of their own (`cparity-c-aarch64.txt`): a compiler upgrade that
//! changes C's code lowers them, and no run may raise them.

use std::{fs, path::{Path, PathBuf}};

use crate::{config::Config, process, step::Verdict};

use crate::cparity::{asm::Instruction, flow};

const KERNELS: &str = "crates/quench-tier-corpus/cparity/c";
/// The AArch64 twins' counts; the x86-64 C column is not recorded yet.
const TABLE: &str = "crates/quench-tier-corpus/cparity-c-aarch64.txt";
const COMPILER: &str = "clang";
/// `-fno-inline` keeps the twin's kernel function a symbol of its own, so its work loop is
/// measured instead of the ``main`` it was inlined into (whose generator loop is a different
/// job).
const FLAGS: [&str; 4] = ["-O2", "-mcpu=apple-m4", "-c", "-fno-inline"];
/// The convention `ir_budget.txt` states for its C column.
const SCALAR_FLAGS: [&str; 2] = ["-fno-vectorize", "-fno-unroll-loops"];

/// One kernel's C counts: instructions per iteration of its work loop, vectorized and scalar.
struct Counts {
    vectorized: Option<u32>,
    scalar: Option<u32>,
}

pub fn run(config: &Config) -> Verdict {
    let directory = config.target.join("c-instructions");
    fs::create_dir_all(&directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    let sources = kernels(&config.root.join(KERNELS))?;
    let table_path = config.root.join(TABLE);
    let recorded = read_table(&table_path);
    let mut counts = Vec::with_capacity(sources.len());
    for (name, source) in &sources {
        let counts_for = |flags: &[&str], suffix: &str| -> Result<Option<u32>, String> {
            let object = directory.join(format!("{name}{suffix}.o"));
            compile(source, &object, flags, config)?;
            hottest(&object).map(|count| Some(count as u32))
        };
        counts.push((name.clone(), Counts {
            vectorized: counts_for(&[], "")?,
            scalar: counts_for(&SCALAR_FLAGS, "-scalar")?,
        }));
    }
    // The ratchet only shrinks: a count above the recorded one fails the step.
    let mut failures = Vec::new();
    for (name, counts) in &counts {
        let limit = recorded.get(name).copied().flatten();
        if let (Some(limit), Some(count)) = (limit, counts.scalar)
            && count > limit
        {
            failures.push(format!("{name} scalar C: {count} instructions exceed its ceiling {limit}"));
        }
    }
    let rendered = render(&counts);
    if fs::read_to_string(&table_path).ok().as_deref() != Some(rendered.as_str()) {
        fs::write(&table_path, &rendered).map_err(|error| format!("{}: {error}", table_path.display()))?;
    }
    if !failures.is_empty() {
        return Err(failures.join("\n"));
    }
    let measured = counts.iter().filter(|(_, counts)| counts.scalar.is_some()).count();
    Ok(format!("{measured} kernels: scalar C {}..{} instructions per iteration, vectorized {}..{}; table {TABLE}",
        counts.iter().filter_map(|(_, c)| c.scalar).min().unwrap_or(0),
        counts.iter().filter_map(|(_, c)| c.scalar).max().unwrap_or(0),
        counts.iter().filter_map(|(_, c)| c.vectorized).min().unwrap_or(0),
        counts.iter().filter_map(|(_, c)| c.vectorized).max().unwrap_or(0)))
}

fn kernels(directory: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let entries = fs::read_dir(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    let mut kernels = Vec::new();
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.extension().is_some_and(|extension| extension == "c")
            && let Some(name) = path.file_stem().and_then(|stem| stem.to_str())
        {
            kernels.push((name.to_owned(), path));
        }
    }
    kernels.sort();
    Ok(kernels)
}

fn compile(source: &Path, object: &Path, extra: &[&str], config: &Config) -> Result<(), String> {
    let mut command = config.command(COMPILER);
    command.args(FLAGS).args(extra).arg(source).arg("-o").arg(object);
    let finished = process::run(&mut command)?;
    if finished.success { Ok(()) } else { Err(format!("{}: {}", source.display(), finished.excerpt())) }
}

/// The instructions `object`'s kernel function spends per iteration of its work loop: its
/// largest innermost loop, or — a kernel with no loop, as `fib` is recursive — the whole
/// function, its per-call cost. The kernel is the largest function that is not `main`; a twin's
/// `main` runs the generator.
fn hottest(object: &Path) -> Result<usize, String> {
    let finished = process::run(&mut std::process::Command::new("objdump")
        .args(["-d", "--no-show-raw-insn"]).arg(object))?;
    if !finished.success {
        return Err(format!("{}: {}", object.display(), finished.excerpt()));
    }
    let functions = functions(&finished.stdout);
    let kernel = functions.iter().filter(|(name, _)| !name.ends_with("main"))
        .max_by_key(|(_, code)| code.len())
        .ok_or_else(|| format!("{}: no kernel function", object.display()))?;
    let body = flow::largest_innermost(&kernel.1).unwrap_or_else(|| kernel.1.clone());
    Ok(body.iter().filter(|instruction| !instruction.is_nop()).count())
}

/// The instructions of each function in an `objdump -d` object listing, by symbol: a label line
/// starts one.
fn functions(text: &str) -> Vec<(String, Vec<Instruction>)> {
    let mut functions: Vec<(String, Vec<Instruction>)> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some((address, rest)) = trimmed.split_once('<')
            && let Some(name) = rest.strip_suffix(">:")
            && !address.trim().is_empty() && address.trim().chars().all(|c| c.is_ascii_hexdigit())
        {
            functions.push((name.to_owned(), Vec::new()));
            continue;
        }
        let Some(instruction) = instruction(line) else { continue };
        if let Some((_, code)) = functions.last_mut() {
            code.push(instruction);
        }
    }
    functions
}

/// One instruction of an `objdump -d` object listing, its symbolic annotation dropped so a
/// branch's operand is the address it goes to.
fn instruction(line: &str) -> Option<Instruction> {
    let (address, rest) = line.trim().split_once(':')?;
    let address = u64::from_str_radix(address.trim_start_matches('<').trim(), 16).ok()?;
    let body = rest.split('<').next()?.trim();
    let (mnemonic, operands) = body.split_once(char::is_whitespace).unwrap_or((body, ""));
    // `; =0` and comments end an instruction's operands.
    let operands = operands.split(';').next().unwrap_or_default().trim();
    (!mnemonic.is_empty()).then(|| Instruction { address, text: format!("{mnemonic} {operands}").trim().to_owned() })
}

fn read_table(path: &Path) -> std::collections::BTreeMap<String, Option<u32>> {
    fs::read_to_string(path).unwrap_or_default().lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?.to_owned();
            Some((name, fields.next().and_then(|field| field.parse().ok())))
        })
        .collect()
}

fn render(counts: &[(String, Counts)]) -> String {
    let mut text = String::from(
        "# C twins of the kernel corpus (crates/quench-tier-corpus/cparity/c), instructions per\n\
         # iteration of the work loop: the largest innermost loop of the object's text, built with\n\
         # `clang -O2 -mcpu=apple-m4` (vectorized) and with `-fno-vectorize -fno-unroll-loops`\n\
         # (scalar, the convention `ir_budget.txt` states for its C column); `-` is not recorded.\n\
         # Counts only shrink. The Quench side is `cparity-aarch64.txt`.\n\
         # kernel vectorized scalar\n");
    for (name, counts) in counts {
        let field = |count: Option<u32>| count.map_or_else(|| "-".to_owned(), |count| count.to_string());
        text.push_str(&format!("{name} {} {}\n", field(counts.vectorized), field(counts.scalar)));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "\n/tmp/x.o:\tfile format mach-o arm64\n\nDisassembly of section __TEXT,__text:\n\n0000000000000000 <ltmp0>:\n       0:      \tcmp\tw2, #0x1\n       4:      \tb.lt\t0xb4 <ltmp0+0xb4>\n       8:      \tmov\tw8, w8\n";

    #[test]
    fn an_object_listing_keeps_addresses_absolute_targets_and_functions() {
        let functions = functions(LISTING);
        assert_eq!(functions.len(), 1);
        let code = &functions[0].1;
        assert_eq!(code.iter().map(|code| code.address).collect::<Vec<_>>(), [0, 4, 8]);
        assert_eq!(code[1].target(), Some(0xb4));
        assert_eq!(code[2].text, "mov w8, w8");
    }
}
