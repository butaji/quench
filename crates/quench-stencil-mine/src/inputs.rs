//! Reads the corpus: JavaScript files and directories compiled to Quench bytecode.
use quench_runtime::{Engine, FlowFunction};
use std::path::{Path, PathBuf};

const EXTENSIONS: &[&str] = &["js", "mjs", "cjs"];

pub struct Compiled {
    pub functions: Vec<FlowFunction>,
    pub files: usize,
    pub failures: Vec<PathBuf>,
}

pub fn collect_files(path: &Path, out: &mut Vec<PathBuf>) {
    let Ok(metadata) = std::fs::metadata(path) else { return };
    if metadata.is_file() {
        out.push(path.to_path_buf());
        return;
    }
    let mut children: Vec<PathBuf> = std::fs::read_dir(path).into_iter().flatten().flatten().map(|entry| entry.path()).collect();
    children.sort();
    for child in children {
        let is_source = child.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.contains(&e));
        if child.is_dir() || is_source {
            collect_files(&child, out);
        }
    }
}

/// Compiles `source` as a script, else as a module.
pub fn compile_source(source: &str, name: &str) -> Option<Vec<FlowFunction>> {
    let program = Engine::specialize(source, name).or_else(|_| Engine::specialize_module(source, name)).ok()?;
    Some(program.flow_functions())
}

pub fn compile_paths(paths: &[PathBuf]) -> Compiled {
    let mut files = Vec::new();
    paths.iter().for_each(|path| collect_files(path, &mut files));
    let mut compiled = Compiled { functions: Vec::new(), files: files.len(), failures: Vec::new() };
    for file in files {
        let functions = std::fs::read_to_string(&file)
            .ok()
            .and_then(|source| compile_source(&source, &file.to_string_lossy()));
        match functions {
            Some(functions) => compiled.functions.extend(functions),
            None => compiled.failures.push(file),
        }
    }
    compiled
}
