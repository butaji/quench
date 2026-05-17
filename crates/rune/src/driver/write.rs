//! # Code Generation Writer
//!
//! Writes generated Rust code to the cache directory with atomic operations.

use super::BuildDriver;
use crate::codegen::emitter::utils::escape_rust_keyword_for_module;
use crate::{codegen::GeneratedModule, Result};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Atomic file writer - writes to temp file then renames.
fn atomic_write(path: &Path, content: &str) -> io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let tmp_path = parent.join(format!(
        ".{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));

    let mut file = fs::File::create(&tmp_path)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    drop(file);

    fs::rename(&tmp_path, path)
}

impl BuildDriver {
    /// Write generated modules to cache with atomic operations.
    pub fn write_generated(&self, modules: &[GeneratedModule]) -> Result<()> {
        let generated_dir = self.cache.generated_dir();
        fs::create_dir_all(&generated_dir)?;

        for module in modules {
            write_single_module(&generated_dir, module)?;
        }

        self.write_mod_files(&generated_dir, modules)?;
        self.setup_cache_cargo()?;

        Ok(())
    }

    /// Write mod.rs files for directory structure.
    fn write_mod_files(&self, cache_dir: &Path, modules: &[GeneratedModule]) -> Result<()> {
        let dirs = collect_unique_dirs(modules);

        for dir in dirs {
            let dir_path = cache_dir.join(&dir);
            fs::create_dir_all(&dir_path)?;
            let mod_rs = dir_path.join("mod.rs");
            if !mod_rs.exists() {
                atomic_write(&mod_rs, "")?;
            }
        }

        Ok(())
    }

    /// Setup Cargo.toml in cache directory.
    pub fn setup_cache_cargo(&self) -> Result<()> {
        let manifest_path = self.cache.generated_cargo_toml();
        let cache_src = self.cache.generated_dir();

        fs::create_dir_all(&cache_src)?;
        self.write_cache_lib(&cache_src)?;

        let crates_path = self.options.workspace.join("crates");
        let cache_to_crates = relative_path(&manifest_path, &crates_path);

        let manifest = generate_manifest(&self.config.build.target_crate, &cache_to_crates);
        atomic_write(&manifest_path, &manifest)?;

        Ok(())
    }

    /// Write the cache lib.rs.
    pub fn write_cache_lib(&self, cache_src: &Path) -> Result<()> {
        let lib_path = cache_src.parent().unwrap().join("lib.rs");
        fs::create_dir_all(lib_path.parent().unwrap())?;

        let generated_dir = cache_src;
        let modules = self.collect_modules(generated_dir)?;

        let lib_content = build_lib_content();
        atomic_write(&lib_path, &lib_content)?;

        self.write_directory_mod_files(generated_dir, &modules)?;

        copy_native_module(self, cache_src)?;

        Ok(())
    }

    /// Collect module names from generated directory.
    fn collect_modules(
        &self,
        dir: &Path,
    ) -> Result<std::collections::HashMap<String, Vec<String>>> {
        let mut modules: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();

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

        Ok(modules)
    }

    /// Write mod.rs files for each directory level.
    fn write_directory_mod_files(
        &self,
        generated_dir: &Path,
        modules: &std::collections::HashMap<String, Vec<String>>,
    ) -> Result<()> {
        write_root_mod_file(generated_dir, modules)?;
        write_subdirectory_mod_files(generated_dir, modules)?;
        Ok(())
    }
}

fn write_single_module(generated_dir: &Path, module: &GeneratedModule) -> Result<()> {
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

fn clean_module_name(name: &str) -> String {
    if std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("r"))
        && name.len() > 1
    {
        String::from(&name[..name.len() - 2])
    } else {
        String::from(name)
    }
}

fn collect_unique_dirs(modules: &[GeneratedModule]) -> std::collections::HashSet<String> {
    let mut dirs = std::collections::HashSet::new();

    for module in modules {
        if let Some(parent) = Path::new(&module.name).parent() {
            if !parent.as_os_str().is_empty() && parent.to_string_lossy() != "." {
                dirs.insert(parent.to_string_lossy().to_string());
            }
        }
    }

    dirs
}

fn generate_manifest(target_crate: &str, cache_to_crates: &str) -> String {
    format!(
        r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[lib]
name = "{}"
crate-type = ["cdylib", "rlib"]
path = "src/lib.rs"

[dependencies]
protocol = {{ path = "{}/protocol" }}
ratatui = "0.26"
crossterm = "0.27"
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"

[workspace]
"#,
        target_crate, target_crate, cache_to_crates
    )
}

fn build_lib_content() -> String {
    String::from(
        r#"//! Generated Rune modules

mod native;

pub mod generated;

use protocol::{App, AppState};
pub use protocol::{Filter, Task};

#[no_mangle]
pub extern "C" fn create_app() -> *mut dyn App {
    Box::into_raw(Box::new(AppImpl::default()))
}

#[derive(Default)]
struct AppImpl;

impl App for AppImpl {
    fn update(&mut self, state: &mut AppState) {
        generated::main::update(state);
    }

    fn render(&self, frame: &mut ratatui::Frame, state: &AppState) {
        let _ = frame;
        let _ = state;
    }

    fn handle_key(&mut self, _key: crossterm::event::KeyEvent, _state: &mut AppState) {}
}
"#,
    )
}

fn process_module_entry(path: &Path, base_dir: &Path) -> Option<(String, String)> {
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

fn write_root_mod_file(
    generated_dir: &Path,
    modules: &std::collections::HashMap<String, Vec<String>>,
) -> Result<()> {
    let root_mod_content: String = modules
        .get("")
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|m| format!("pub mod {};", m))
        .collect::<Vec<_>>()
        .join("\n");
    atomic_write(&generated_dir.join("mod.rs"), &root_mod_content)?;
    Ok(())
}

fn write_subdirectory_mod_files(
    generated_dir: &Path,
    modules: &std::collections::HashMap<String, Vec<String>>,
) -> Result<()> {
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

fn copy_native_module(driver: &BuildDriver, cache_src: &Path) -> Result<()> {
    let native_src = driver
        .options
        .workspace
        .join("crates")
        .join(&driver.config.build.target_crate)
        .join("src/native");
    let native_dest = cache_src.parent().unwrap().join("native");

    if native_src.exists() {
        copy_dir_recursive(&native_src, &native_dest)?;
    } else {
        fs::create_dir_all(&native_dest)?;
        atomic_write(&native_dest.join("mod.rs"), "// Native modules\n")?;
    }
    Ok(())
}

/// Calculate relative path from a file to a target directory.
fn relative_path(from_file: &Path, to_target: &Path) -> String {
    let Some(from_dir) = from_file.parent() else {
        return to_target.to_string_lossy().to_string();
    };

    let mut ups = 0;
    let mut current = from_dir;
    while !to_target.starts_with(current) {
        if let Some(parent) = current.parent() {
            ups += 1;
            current = parent;
        } else {
            break;
        }
    }

    let mut parts: Vec<String> = vec![String::from(".."); ups];

    if let Ok(rest) = to_target.strip_prefix(current) {
        for component in rest.components() {
            if let std::path::Component::Normal(s) = component {
                parts.push(s.to_string_lossy().into_owned());
            }
        }
    }

    parts.join("/")
}

/// Copy directory recursively.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;

    for entry in walkdir::WalkDir::new(src)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let rel_path = path
            .strip_prefix(src)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        let dest_path = dst.join(rel_path);

        if path.is_dir() {
            fs::create_dir_all(&dest_path)?;
        } else {
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(path, &dest_path)?;
        }
    }

    Ok(())
}
