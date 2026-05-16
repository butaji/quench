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
        let parts: Vec<&str> = name.split('/').collect();
        let last = parts.last().unwrap_or(&name);

        // The name is the file stem, which for Rune files is like:
        // - "main.r" for "main.r.ts" or "main.r.tsx"
        // - "handlers/keyboard.r" for "handlers/keyboard.r.ts"
        // We need to strip the ".r" suffix to get the proper module name
        let clean = if std::path::Path::new(last)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("r"))
            && last.len() > 1
        {
            &last[..last.len() - 2]  // Remove trailing ".r"
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
        // Cache is at workspace/target/rune-cache, crates is at workspace/crates
        // So we need to go up 2 levels (target, then to workspace) then down to crates
        let cache_to_crates = if self.cache.root().strip_prefix(self.options.workspace.as_path()).is_ok() {
            // Count directory depth from workspace to cache root
            let rel_path = self.cache.root().strip_prefix(self.options.workspace.as_path())
                .unwrap_or_else(|_| std::path::Path::new(""));
            let depth = rel_path.components().count();
            // "../" goes up one level, so repeat 'depth' times to go up 'depth' levels
            "../".repeat(depth) + "crates"
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
        // Clone state for the generated update function if it takes ownership
        let mut state_copy = state.clone();
        generated::main::update(state_copy);
        // Copy fields back (simplified - assumes flat state)
        *state = state_copy;
    }

    fn render(&self, frame: &mut ratatui::Frame, state: &AppState) {
        // Render is handled by the generated view functions
        let _ = frame;
        let _ = state;
    }

    fn handle_key(&mut self, _key: crossterm::event::KeyEvent, _state: &mut AppState) {}
}
"#,
        );

        std::fs::write(&lib_path, lib_content)?;

        // Write mod.rs files for each directory level
        self.write_directory_mod_files(generated_dir, &modules)?;

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

    /// Escape a Rust keyword for use as a module name.
    fn escape_rust_keyword(name: &str) -> String {
        match name {
            "as" | "async" | "await" | "break" | "const" | "continue" | "crate"
            | "dyn" | "else" | "enum" | "extern" | "false" | "fn" | "for" | "if"
            | "impl" | "in" | "let" | "loop" | "match" | "mod" | "move" | "mut"
            | "pub" | "ref" | "return" | "self" | "Self" | "static" | "struct"
            | "super" | "trait" | "true" | "type" | "unsafe" | "use" | "where"
            | "while" => format!("r#{name}"),
            _ => name.to_string(),
        }
    }

    /// Collect module names from generated directory.
    /// Returns a map of directory -> list of module names in that directory.
    fn collect_modules(&self, dir: &Path) -> Result<std::collections::HashMap<String, Vec<String>>> {
        let mut modules: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();

        for entry in walkdir::WalkDir::new(dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "rs") {
                // Get relative path from cache dir
                if let Ok(rel) = path.strip_prefix(dir) {
                    let rel_str = rel.to_string_lossy();
                    // Remove .rs extension
                    let module_name = rel_str.replace(".rs", "");
                    
                    // Get parent directory (relative to cache dir)
                    let parent = Path::new(&module_name).parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    
                    // Get the module name (last component)
                    let stem = Path::new(&module_name)
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    
                    // For mod.rs files, the module is just "mod", not escaped
                    // The file mod.rs defines the module named "mod"
                    let module_decl = if stem == "mod" {
                        "mod".to_string()
                    } else {
                        Self::escape_rust_keyword(&stem)
                    };
                    
                    // Add to parent directory's module list
                    modules
                        .entry(parent)
                        .or_default()
                        .push(module_decl);
                }
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
        // Write mod.rs for the root generated directory
        let root_mod_content: String = modules
            .get("")
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|m| format!("pub mod {};", m))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(generated_dir.join("mod.rs"), root_mod_content)?;

        // Write mod.rs for each subdirectory
        for (dir, mods) in modules {
            if dir.is_empty() {
                continue;
            }
            let dir_path = generated_dir.join(dir);
            std::fs::create_dir_all(&dir_path)?;
            let mod_content: String = mods
                .iter()
                .map(|m| format!("pub mod {};", m))
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(dir_path.join("mod.rs"), mod_content)?;
        }

        Ok(())
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
