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
        let dirs = write_impl::modules::collect_unique_dirs(modules);
        for dir in &dirs {
            write_impl::modules::create_dir_mod_file(generated_dir, dir)?;
        }
        Ok(())
    }
}
