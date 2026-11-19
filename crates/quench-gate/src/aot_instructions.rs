//! Step: the produced image's own ahead bodies, measured per iteration.
//!
//! `c-parity` measures what a tier's compiled loop spends; this step measures what an *image*
//! spends, from the image's own machine code: each kernel of the corpus is built
//! (`quench-node build … --dynamic-source --keep-symbols`), the `_quench_unit_*_body` symbols
//! are located with `nm`, each body's bytes are sliced out of `__TEXT` and disassembled by the
//! host's disassembler (`cparity::asm`), and the executed path of the body's largest innermost
//! loop is counted (`cparity::flow::iteration`). That count is the AOT tier's instructions per
//! iteration, compared with the C twin's scalar loop from `cparity-c-aarch64.txt`.
//!
//! A body that does not lower the kernel's loop reports none: the ahead code hands the loop
//! back, and the number is exactly that fact. The table is a shrink-only ratchet
//! (`aot-instructions-aarch64.txt`).
//!
//! Alone: `quench-gate --only aot-instructions`.

use std::{collections::BTreeMap, fs, path::{Path, PathBuf}, process::Command};

use crate::{build::NODE, config::Config, step::Verdict};

use crate::cparity::{asm, flow};

const KERNELS: &str = "crates/quench-tier-corpus/cparity/js";
const C_TABLE: &str = "crates/quench-tier-corpus/cparity-c-aarch64.txt";
const TABLE: &str = "crates/quench-tier-corpus/aot-instructions-aarch64.txt";
const BODY_PREFIX: (&str, &str) = ("_quench_unit_", "_body");
const TEXT_SEGMENT: &str = "__TEXT";

/// One kernel's ahead code: its hottest loop's executed path, or none when the body lowers none.
#[derive(Clone, Copy)]
struct Ahead {
    /// How many functions the image lowers ahead: its `_quench_unit_*_body` symbols. One means
    /// only the entry program's top level, with every called function compiled at run time.
    bodies: u32,
    instructions: Option<u32>,
}

pub fn run(config: &Config) -> Verdict {
    let host = config.binary(NODE)?;
    let directory = config.target.join("aot-instructions");
    fs::create_dir_all(&directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    let sources = kernels(&config.root.join(KERNELS))?;
    let c_costs = c_costs(&config.root.join(C_TABLE))?;
    let recorded = read_table(&config.root.join(TABLE));
    let mut rows = Vec::with_capacity(sources.len());
    for (name, source) in &sources {
        let image = directory.join(name);
        let output = Command::new(&host).arg("build").arg(source).arg("-o").arg(&image)
            .arg("--dynamic-source").arg("--keep-symbols").output().map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(format!("{name}: build failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
        }
        rows.push((name.clone(), measure(&image)));
    }
    // The ratchet: a count may fall, never rise.
    let mut failures = Vec::new();
    for (name, ahead) in &rows {
        if let (Some(limit), Some(count)) = (recorded.get(name).copied().flatten(), ahead.instructions)
            && count > limit
        {
            failures.push(format!("{name}: {count} instructions per iteration exceed its ceiling {limit}"));
        }
    }
    let rendered = render(&rows, &c_costs);
    if fs::read_to_string(&config.root.join(TABLE)).ok().as_deref() != Some(rendered.as_str()) {
        fs::write(&config.root.join(TABLE), rendered).map_err(|error| error.to_string())?;
    }
    if !failures.is_empty() {
        return Err(failures.join("\n"));
    }
    let lowered = rows.iter().filter(|(_, ahead)| ahead.instructions.is_some()).count();
    Ok(format!("{lowered} of {} kernels run their loop in ahead code; table {TABLE}", rows.len()))
}

/// The instructions per iteration of the hottest loop in `image`'s ahead bodies, if one lowers it.
fn measure(image: &Path) -> Ahead {
    let bytes = match fs::read(image) { Ok(bytes) => bytes, Err(_) => return Ahead { bodies: 0, instructions: None } };
    let Some(text_base) = text_base(image) else { return Ahead { bodies: 0, instructions: None } };
    let Some(symbols) = symbols(image) else { return Ahead { bodies: 0, instructions: None } };
    let bodies = u32::try_from(symbols.iter().filter(|(_, name)|
        name.starts_with(BODY_PREFIX.0) && name.ends_with(BODY_PREFIX.1)).count()).unwrap_or(u32::MAX);
    let mut best: Option<u32> = None;
    for (index, (address, name)) in symbols.iter().enumerate() {
        if !(name.starts_with(BODY_PREFIX.0) && name.ends_with(BODY_PREFIX.1)) { continue; }
        let end = symbols.get(index + 1).map_or(*address + 0x4000, |(next, _)| *next);
        let (start_offset, length) = match (address.checked_sub(text_base), end.checked_sub(*address)) {
            (Some(start), Some(length)) if length > 0 => (start as usize, length as usize),
            _ => continue,
        };
        let Some(slice) = bytes.get(start_offset..start_offset + length) else { continue };
        let scratch = std::env::temp_dir().join(format!("quench-aot-body-{}", std::process::id()));
        if fs::write(&scratch, slice).is_err() { continue; }
        let code = asm::disassemble(&scratch).unwrap_or_default();
        let _ = fs::remove_file(&scratch);
        let Some(body) = flow::hottest(&code).or_else(|| flow::largest_innermost(&code)) else { continue };
        let count = u32::try_from(body.iter().filter(|instruction| !instruction.is_nop()).count()).unwrap_or(u32::MAX);
        if body.len() > 1 { best = Some(best.map_or(count, |best| best.max(count))); }
    }
    Ahead { bodies, instructions: best }
}

/// The `__TEXT` segment's vmaddr: the produced image maps its first section at file offset zero.
fn text_base(image: &Path) -> Option<u64> {
    let output = Command::new("otool").arg("-l").arg(image).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut in_text = false;
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix("segname ") {
            in_text = name.trim() == TEXT_SEGMENT;
            continue;
        }
        if in_text && let Some(value) = line.strip_prefix("vmaddr ") {
            return u64::from_str_radix(value.trim().trim_start_matches("0x"), 16).ok();
        }
    }
    None
}

/// `nm -n`'s defined symbols of `image`, ascending.
fn symbols(image: &Path) -> Option<Vec<(u64, String)>> {
    let output = Command::new("nm").arg("-n").arg(image).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut symbols = Vec::new();
    for line in text.lines() {
        let mut fields = line.splitn(3, ' ');
        let (Some(address), Some(_kind), Some(name)) = (fields.next(), fields.next(), fields.next()) else { continue };
        let Ok(address) = u64::from_str_radix(address, 16) else { continue };
        symbols.push((address, name.trim().to_owned()));
    }
    symbols.sort();
    Some(symbols)
}

fn kernels(directory: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let entries = fs::read_dir(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    let mut kernels = Vec::new();
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.extension().is_some_and(|extension| extension == "js")
            && let Some(name) = path.file_stem().and_then(|stem| stem.to_str())
        {
            kernels.push((name.to_owned(), path));
        }
    }
    kernels.sort();
    Ok(kernels)
}

/// The C twin's scalar work loop per kernel (`cparity-c-aarch64.txt`).
fn c_costs(path: &Path) -> Result<BTreeMap<String, u32>, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(text.lines().filter(|line| !line.starts_with('#')).filter_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        Some(((*fields.first()?).to_owned(), fields.get(2)?.parse().ok()?))
    }).collect())
}

fn read_table(path: &Path) -> BTreeMap<String, Option<u32>> {
    fs::read_to_string(path).unwrap_or_default().lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some((fields.next()?.to_owned(), fields.next().and_then(|field| field.parse().ok())))
        })
        .collect()
}

fn render(rows: &[(String, Ahead)], c_costs: &BTreeMap<String, u32>) -> String {
    let mut text = String::from(
        "# The produced image's ahead code (`_quench_unit_*_body`), instructions per iteration of\n\
         # its hottest loop, against the C twin's scalar loop (`cparity-c-aarch64.txt`); `-` is a\n\
         # body that lowers no loop, i.e. the image hands that loop back. Measured by building each\n\
         # kernel with `quench-node build --dynamic-source --keep-symbols` and disassembling the\n\
         # body (`quench-gate --only aot-instructions`). Counts only shrink.\n\
         # `bodies` counts the functions the image lowers ahead; `aot` is the hottest loop of
\
         # those bodies. A kernel whose own function has no body shows its top level's loop, so
\
         # its `aot` is not the kernel loop: the image compiles that at run time.\n\
         # kernel bodies aot c-aot\n");
    for (name, ahead) in rows {
        let instructions = ahead.instructions.map_or_else(|| "-".to_owned(), |count| count.to_string());
        let bodies = ahead.bodies.to_string();
        let c = c_costs.get(name).map_or_else(|| "-".to_owned(), |count| count.to_string());
        text.push_str(&format!("{name} {bodies} {instructions} {c}\n"));
    }
    text
}
