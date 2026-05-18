//! # Code Generation Writer
//!
//! Writes generated Rust code to the cache directory with atomic operations.

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
    /// Write generated modules to cache with atomic operations.
    pub fn write_generated(&self, modules: &[GeneratedModule]) -> Result<()> {
        let generated_dir = self.cache.generated_dir();
        fs::create_dir_all(&generated_dir)?;

        for module in modules {
            write_impl::modules::write_single_module(&generated_dir, module)?;
        }

        self.write_mod_files(&generated_dir, modules)?;
        self.setup_cache_cargo()?;

        Ok(())
    }

    /// Write mod.rs files for directory structure.
    fn write_mod_files(&self, cache_dir: &Path, modules: &[GeneratedModule]) -> Result<()> {
        let dirs = write_impl::modules::collect_unique_dirs(modules);
        for dir in &dirs {
            write_impl::modules::create_dir_mod_file(cache_dir, dir)?;
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
        let cache_to_crates = write_impl::paths::relative_path(&manifest_path, &crates_path);

        let manifest = write_impl::manifest::generate_manifest(
            &self.config.build.target_crate,
            &cache_to_crates,
        );
        atomic_write(&manifest_path, &manifest)?;

        Ok(())
    }

    /// Write the cache lib.rs.
    pub fn write_cache_lib(&self, cache_src: &Path) -> Result<()> {
        // Hoist parent path computation once
        let Some(cache_parent) = cache_src.parent() else {
            return Err(crate::RunieError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid cache path",
            )));
        };

        let lib_path = cache_parent.join("lib.rs");
        fs::create_dir_all(cache_parent)?;

        let generated_dir = cache_src;
        let modules = write_impl::modules::collect_modules(generated_dir);

        let lib_content = write_impl::manifest::generate_lib_content();
        atomic_write(&lib_path, &lib_content)?;

        write_impl::modules::write_directory_mod_files(generated_dir, &modules)?;

        self.copy_native_module(cache_parent)?;

        Ok(())
    }

    /// Copy native module to cache.
    fn copy_native_module(&self, cache_parent: &Path) -> Result<()> {
        let native_src = self
            .options
            .workspace
            .join("crates")
            .join(&self.config.build.target_crate)
            .join("src/native");
        let native_dest = cache_parent.join("native");

        if native_src.exists() {
            write_impl::paths::copy_dir_recursive(&native_src, &native_dest)?;
        } else {
            fs::create_dir_all(&native_dest)?;
            atomic_write(&native_dest.join("mod.rs"), "// Native modules\n")?;
        }
        Ok(())
    }
}
