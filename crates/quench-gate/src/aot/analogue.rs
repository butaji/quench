//! The C analogues the AOT step measures. Each `quench-bench/c-analogues/*.c` names in its first
//! three lines the JetStream row it mirrors, the row's files in load order, and the iteration
//! count its C main runs by default.

use std::{
    fs,
    path::{Path, PathBuf},
};

pub const DIRECTORY: &str = "quench-bench/c-analogues";
const SOURCE_EXTENSION: &str = "c";
/// The lines a header takes, from the top of the file.
const HEADER_LINES: usize = 3;
const ROW_KEY: &str = "// jetstream-row: ";
const FILES_KEY: &str = "// jetstream-files: ";
const ITERATIONS_KEY: &str = "// iterations: ";

/// One analogue: its C source and the JetStream row it mirrors.
#[derive(Debug)]
pub struct Analogue {
    pub row: String,
    /// The row's files, relative to the JetStream submodule, in load order.
    pub files: Vec<String>,
    pub iterations: u64,
    pub source: PathBuf,
}

/// The rest of the header line that starts with `key`.
fn field<'a>(header: &[&'a str], key: &str) -> Result<&'a str, String> {
    header.iter().find_map(|line| line.strip_prefix(key)).ok_or_else(|| format!("no `{}` header line", key.trim_end()))
}

impl Analogue {
    /// Reads the header of the C source `text` at `source`.
    pub fn parse(source: &Path, text: &str) -> Result<Self, String> {
        let header: Vec<&str> = text.lines().take(HEADER_LINES).collect();
        let row = field(&header, ROW_KEY)?.trim().to_owned();
        let files: Vec<String> = field(&header, FILES_KEY)?.split_whitespace().map(str::to_owned).collect();
        let iterations: u64 = field(&header, ITERATIONS_KEY)?.trim().parse().map_err(|error| format!("iterations: {error}"))?;
        if row.is_empty() || files.is_empty() {
            return Err(format!("{}: the header names no row or no files", source.display()));
        }
        Ok(Self { row, files, iterations, source: source.to_owned() })
    }
}

/// Every analogue in `root`, in name order.
pub fn discover(root: &Path) -> Result<Vec<Analogue>, String> {
    let entries = fs::read_dir(root).map_err(|error| format!("{}: {error}", root.display()))?;
    let mut sources = Vec::new();
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.extension().is_some_and(|extension| extension == SOURCE_EXTENSION) {
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
