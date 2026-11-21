//! The C analogues the AOT step measures. Each `quench-bench/c-analogues/*.c` (or `.cc` for C++
//! workloads) names in its first four lines the JetStream row it mirrors, the row's files in load
//! order, the iteration count its C main runs by default, and the C/C++ sources and compiler flags
//! it links (`// c-sources:`, optional, paths relative to the JetStream root).

use std::{
    fs,
    path::{Path, PathBuf},
};

use super::driver::Driver;

pub const DIRECTORY: &str = "quench-bench/c-analogues";
/// The extensions of analogue drivers: `c`, and `cc` for an analogue whose workload is C++.
const SOURCE_EXTENSIONS: [&str; 2] = ["c", "cc"];
/// The lines a header takes, from the top of the file.
const HEADER_LINES: usize = 8;
const ROW_KEY: &str = "// jetstream-row: ";
const FILES_KEY: &str = "// jetstream-files: ";
const ITERATIONS_KEY: &str = "// iterations: ";
const C_SOURCES_KEY: &str = "// c-sources: ";
const DRIVER_KEY: &str = "// jetstream-driver: ";

/// One analogue: its C driver, the JetStream row it mirrors, and what the driver links.
#[derive(Debug)]
pub struct Analogue {
    pub row: String,
    /// The row's files, relative to the JetStream submodule, in load order.
    pub files: Vec<String>,
    pub iterations: u64,
    pub source: PathBuf,
    /// The `// c-sources:` tokens in order: source paths relative to the JetStream submodule, and
    /// the flags cc takes (`-D…`, `-I…` in attached form, `-l…`). Empty when the header has none.
    pub c_sources: Vec<String>,
    /// How JetStream's driver runs the row (`// jetstream-driver:`; the default when absent).
    pub driver: Driver,
}

/// The rest of the header line that starts with `key`, if the header has one.
fn optional_field<'a>(header: &[&'a str], key: &str) -> Option<&'a str> {
    header.iter().find_map(|line| line.strip_prefix(key))
}

/// The rest of the header line that starts with `key`; the header must have one.
fn field<'a>(header: &[&'a str], key: &str) -> Result<&'a str, String> {
    optional_field(header, key).ok_or_else(|| format!("no `{}` header line", key.trim_end()))
}

impl Analogue {
    /// Reads the header of the C source `text` at `source`.
    pub fn parse(source: &Path, text: &str) -> Result<Self, String> {
        let header: Vec<&str> = text.lines().take(HEADER_LINES).collect();
        let row = field(&header, ROW_KEY)?.trim().to_owned();
        let files: Vec<String> = field(&header, FILES_KEY)?.split_whitespace().map(str::to_owned).collect();
        let iterations: u64 = field(&header, ITERATIONS_KEY)?.trim().parse().map_err(|error| format!("iterations: {error}"))?;
        let c_sources: Vec<String> = optional_field(&header, C_SOURCES_KEY).unwrap_or_default().split_whitespace().map(str::to_owned).collect();
        let driver = Driver::parse(optional_field(&header, DRIVER_KEY).unwrap_or_default())?;
        if row.is_empty() || files.is_empty() {
            return Err(format!("{}: the header names no row or no files", source.display()));
        }
        Ok(Self { row, files, iterations, source: source.to_owned(), c_sources, driver })
    }
}

/// Every analogue in `root`, in name order.
pub fn discover(root: &Path) -> Result<Vec<Analogue>, String> {
    let entries = fs::read_dir(root).map_err(|error| format!("{}: {error}", root.display()))?;
    let mut sources = Vec::new();
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.extension().is_some_and(|extension| SOURCE_EXTENSIONS.iter().any(|known| extension == *known)) {
            sources.push(path);
        }
    }
    sources.sort();
    sources
        .iter()
        .map(|source| {
            let text = fs::read_to_string(source).map_err(|error| format!("{}: {error}", source.display()))?;
            Analogue::parse(source, &text)
        })
        .collect()
}
