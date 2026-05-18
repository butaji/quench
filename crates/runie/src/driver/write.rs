//! # Code Generation Writer
//!
//! Writes generated Rust code to src/generated/.

mod write_impl {
    pub mod atomic;
    pub mod manifest;
    pub mod modules;
    pub mod paths;
}

use super::BuildDriver;
use crate::{codegen::GeneratedModule, Result};
use std::fs;
use std::path::Path;

pub use write_impl::atomic::atomic_write;

/// Clean module name for Rust mod declaration.
fn clean_module_name_for_mod(name: &str) -> String {
    // Remove path prefixes
    let name = name.split('/').last().unwrap_or(name);
    // Remove .r.ts, .r.tsx, or trailing .r
    let name = name.strip_suffix(".r.tsx")
        .or_else(|| name.strip_suffix(".r.ts"))
        .or_else(|| name.strip_suffix(".r"))
        .unwrap_or(name);
    name.to_string()
}

impl BuildDriver {
    /// Write generated modules to src/generated/.
    pub fn write_generated(&self, modules: &[GeneratedModule]) -> Result<()> {
        let generated_dir = self.cache.generated_dir();
        fs::create_dir_all(&generated_dir)?;

        for module in modules {
            write_impl::modules::write_single_module(&generated_dir, module)?;
        }

        self.write_mod_files(&generated_dir, modules)?;

        Ok(())
    }

    /// Write mod.rs files for directory structure.
    fn write_mod_files(&self, generated_dir: &Path, modules: &[GeneratedModule]) -> Result<()> {
        // Create root mod.rs with all generated modules
        let mut mod_content = String::new();
        for module in modules {
            // Clean module name: remove .r.ts/.r.tsx extensions, convert paths
            let clean = clean_module_name_for_mod(&module.name);
            mod_content.push_str(&format!("pub mod {};\n", clean));
        }
        atomic_write(&generated_dir.join("mod.rs"), &mod_content)?;
        
        let dirs = write_impl::modules::collect_unique_dirs(modules);
        for dir in &dirs {
            write_impl::modules::create_dir_mod_file(generated_dir, dir)?;
        }
        Ok(())
    }
}
