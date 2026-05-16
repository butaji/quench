//! # Code Generation Writer
//!
//! Writes generated Rust code to the cache directory.

use std::path::Path;
use crate::{Result, codegen::GeneratedModule};
use super::BuildDriver;

impl BuildDriver {
    /// Write generated modules to cache.
    pub fn write_generated(&self, modules: &[GeneratedModule]) -> Result<()> {
        let generated_dir = self.cache.generated_dir();

        // Ensure cache directory exists
        std::fs::create_dir_all(&generated_dir)?;

        for module in modules {
            // Extract relative path from source file
            let rel_path = self.cache_relative_path(&module.name);
            let out_path = generated_dir.join(&rel_path);
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out_path, &module.source)?;
        }

        // Write mod.rs files for directory structure
        self.write_mod_files(&generated_dir, modules)?;

        // Create/refresh Cargo.toml in cache
        self.setup_cache_cargo()?;

        Ok(())
    }

    /// Create relative path for generated file.
    fn cache_relative_path(&self, name: &str) -> String {
        // Handle paths like "main.r", "views/main.r", etc.
        // where the stem already includes the .r suffix from file_stem()
        // We need to strip that and produce "main.rs", "views/main.rs"
        
        let parts: Vec<&str> = name.split('/').collect();
        let last = parts.last().unwrap_or(&name);
        
        // The name might be "main.r" (from file_stem of main.r.ts)
        // or "views/main.r" (for files in subdirectories)
        // We need to convert "main.r" to "main.rs"
        let clean = if last.ends_with(".r") {
            &last[..last.len() - 2]
        } else {
            last
        };
        
        if parts.len() > 1 {
            let dir_parts = &parts[..parts.len() - 1];
            format!("{}/{}.rs", dir_parts.join("/"), clean)
        } else {
            format!("{}.rs", clean)
        }
    }

    /// Write mod.rs files for directory structure.
    fn write_mod_files(
        &self,
        cache_dir: &Path,
        modules: &[GeneratedModule],
    ) -> Result<()> {
        // Collect unique directories
        let mut dirs: std::collections::HashSet<String> = std::collections::HashSet::new();

        for module in modules {
            if let Some(parent) = Path::new(&module.name).parent() {
                if !parent.as_os_str().is_empty()
                    && parent.to_string_lossy() != "."
                {
                    dirs.insert(parent.to_string_lossy().to_string());
                }
            }
        }

        // Write mod.rs for each directory
        for dir in dirs {
            let dir_path = cache_dir.join(&dir);
            std::fs::create_dir_all(&dir_path)?;

            let mod_rs = dir_path.join("mod.rs");
            if !mod_rs.exists() {
                std::fs::write(&mod_rs, "")?;
            }
        }

        Ok(())
    }

    /// Setup Cargo.toml in cache directory.
    pub fn setup_cache_cargo(&self) -> Result<()> {
        let manifest_path = self.cache.generated_cargo_toml();
        let cache_src = self.cache.generated_dir();

        // Create src directory in cache
        std::fs::create_dir_all(&cache_src)?;

        // Generate lib.rs with generated modules
        self.write_cache_lib(&cache_src)?;

        // Calculate relative path from cache to crates
        let cache_to_crates = if self.cache.root().strip_prefix(self.options.workspace.as_path()).is_ok() {
            "../crates".to_string()
        } else {
            // Fallback: absolute path
            self.options.workspace
                .join("crates")
                .to_string_lossy()
                .to_string()
        };

        // Create a completely standalone manifest for the cache
        let manifest = format!(r#"[package]
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
            self.config.build.target_crate,
            self.config.build.target_crate,
            cache_to_crates
        );
        std::fs::write(&manifest_path, manifest)?;

        Ok(())
    }

    /// Write the cache lib.rs.
    pub fn write_cache_lib(&self, cache_src: &Path) -> Result<()> {
        // cache_src is the generated dir (.rune-cache/src/generated)
        // We need to write lib.rs to .rune-cache/src/lib.rs
        let lib_path = cache_src.parent().unwrap().join("lib.rs");

        // Ensure the src directory exists
        std::fs::create_dir_all(lib_path.parent().unwrap())?;

        // Get all generated modules
        let generated_dir = cache_src;
        let modules = self.collect_modules(generated_dir)?;

        let mut lib_content = String::new();
        lib_content.push_str("//! Generated Rune modules\n\n");

        // Add native module (from original crate)
        lib_content.push_str("mod native;\n\n");

        // Add generated module declaration
        lib_content.push_str("pub mod generated;\n\n");

        // Add protocol imports and AppState re-exports
        lib_content.push_str("use protocol::{App, AppState};\n\n");
        lib_content.push_str("// Re-export types\n");
        lib_content.push_str("pub use protocol::{Filter, Task};\n\n");

        // Add create_app function
        lib_content.push_str(
            r#"
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
        let widget = generated::root::render(state);
        frame.render_widget(widget, frame.size());
    }

    fn handle_key(&mut self, _key: crossterm::event::KeyEvent, _state: &mut AppState) {}
}
"#,
        );

        std::fs::write(&lib_path, lib_content)?;

        // Write generated/mod.rs
        let mod_rs_content: String = modules
            .iter()
            .map(|m| {
                let name = m.replace(".rs", "").replace('-', "_");
                format!("pub mod {};", name)
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(generated_dir.join("mod.rs"), mod_rs_content)?;

        // Copy native module from original crate if exists
        let native_src = self.options.workspace
            .join("crates")
            .join(&self.config.build.target_crate)
            .join("src/native");
        let native_dest = cache_src.parent().unwrap().join("native");

        if native_src.exists() {
            copy_dir_recursive(&native_src, &native_dest)?;
        } else {
            std::fs::create_dir_all(&native_dest)?;
            std::fs::write(native_dest.join("mod.rs"), "// Native modules\n")?;
        }

        Ok(())
    }

    /// Collect module names from generated directory.
    fn collect_modules(&self, dir: &Path) -> Result<Vec<String>> {
        let mut modules = Vec::new();

        for entry in walkdir::WalkDir::new(dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "rs") {
                // Get relative path from cache dir
                if let Ok(rel) = path.strip_prefix(dir) {
                    let rel_str = rel.to_string_lossy()
                        .replace(['/', '\\'], "::")
                        .replace(".rs", "");
                    modules.push(rel_str);
                }
            }
        }

        Ok(modules)
    }
}

/// Copy directory recursively.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;

    for entry in walkdir::WalkDir::new(src)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let rel_path =
            path.strip_prefix(src).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        let dest_path = dst.join(rel_path);

        if path.is_dir() {
            std::fs::create_dir_all(&dest_path)?;
        } else {
            if let Some(parent) = dest_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(path, &dest_path)?;
        }
    }

    Ok(())
}
