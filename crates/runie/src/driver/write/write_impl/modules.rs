//! # Module Writer
//!
//! Writes module files and manages directory structure.

use crate::codegen::emitter::utils::escape_rust_keyword_for_module;
use crate::codegen::GeneratedModule;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use super::atomic::atomic_write;
use super::paths::clean_module_name;

/// Collect unique directories from module names.
#[must_use]
pub fn collect_unique_dirs(modules: &[GeneratedModule]) -> HashSet<String> {
    let mut dirs = HashSet::new();

    for module in modules {
        if let Some(parent) = Path::new(&module.name).parent() {
            if !parent.as_os_str().is_empty() && parent.to_string_lossy() != "." {
                dirs.insert(parent.to_string_lossy().to_string());
            }
        }
    }

    dirs
}

/// Create a directory with mod.rs if needed.
pub fn create_dir_mod_file(cache_dir: &Path, dir: &str) -> std::io::Result<()> {
    let dir_path = cache_dir.join(dir);
    fs::create_dir_all(&dir_path)?;
    let mod_rs = dir_path.join("mod.rs");
    if !mod_rs.exists() {
        atomic_write(&mod_rs, "")?;
    }
    Ok(())
}

/// Write a single generated module to file.
pub fn write_single_module(generated_dir: &Path, module: &GeneratedModule) -> crate::Result<()> {
    let rel_path = module
        .name
        .split('/')
        .next_back()
        .unwrap_or(module.name.as_str());
    let clean = clean_module_name(rel_path);
    let out_path = generated_dir.join(format!("{}.rs", clean));

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write(&out_path, &module.source)?;
    Ok(())
}

/// Process a file entry to extract module info.
#[must_use]
pub fn process_module_entry(path: &Path, base_dir: &Path) -> Option<(String, String)> {
    if !path.is_file() || path.extension().is_some_and(|e| e != "rs") {
        return None;
    }

    let file_name = path.file_name()?.to_str()?;
    if file_name == "mod.rs" {
        return None;
    }

    let rel = path.strip_prefix(base_dir).ok()?;
    let module_name = rel.to_string_lossy().replace(".rs", "");
    let parent = Path::new(&module_name)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let stem = Path::new(&module_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    Some((parent, escape_rust_keyword_for_module(&stem)))
}

/// Write mod.rs for root generated directory.
pub fn write_root_mod_file(
    generated_dir: &Path,
    modules: &HashMap<String, Vec<String>>,
) -> crate::Result<()> {
    let mut content = String::new();

    // Module declarations
    let mod_decls: String = modules
        .get("")
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|m| format!("pub mod {};", m))
        .collect::<Vec<_>>()
        .join("\n");
    content.push_str(&mod_decls);

    atomic_write(&generated_dir.join("mod.rs"), &content)?;
    Ok(())
}

/// Write mod.rs for subdirectories.
pub fn write_subdirectory_mod_files(
    generated_dir: &Path,
    modules: &HashMap<String, Vec<String>>,
) -> crate::Result<()> {
    for (dir, mods) in modules {
        if dir.is_empty() {
            continue;
        }
        let dir_path = generated_dir.join(dir);
        fs::create_dir_all(&dir_path)?;
        let mod_content: String = mods
            .iter()
            .map(|m| format!("pub mod {};", m))
            .collect::<Vec<_>>()
            .join("\n");
        atomic_write(&dir_path.join("mod.rs"), &mod_content)?;
    }
    Ok(())
}

/// Write mod.rs files for each directory level.
pub fn write_directory_mod_files(
    generated_dir: &Path,
    modules: &HashMap<String, Vec<String>>,
) -> crate::Result<()> {
    write_root_mod_file(generated_dir, modules)?;
    write_subdirectory_mod_files(generated_dir, modules)?;
    Ok(())
}

/// Collect module names from generated directory.
#[must_use]
pub fn collect_modules(dir: &Path) -> HashMap<String, Vec<String>> {
    let mut modules: HashMap<String, Vec<String>> = HashMap::new();

    for entry in walkdir::WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if let Some(result) = process_module_entry(path, dir) {
            modules.entry(result.0).or_default().push(result.1);
        }
    }

    modules
}
