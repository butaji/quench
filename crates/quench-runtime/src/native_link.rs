//! Host-neutral native executable publication, available only in compiler builds.

mod exports;
mod footprint;
mod link;
mod payload;
pub use exports::{DynamicInterface, ExportPolicy};
pub use link::{NativeLinkAudit, NativeSupport, checked_output};
pub use footprint::NativeImageFootprint;
pub use payload::{PayloadReduction, remove_offline_symbols};

use std::{collections::BTreeSet, path::{Path, PathBuf}};

pub const CATALOG_FILENAME: &str = "catalog.rs";
pub const REQUIRED_EXPORTS_FILENAME: &str = "required_exports.txt";

/// Compiler-emitted objects and catalog, plus exports owned by the embedding host.
pub struct NativeImageInputs {
    directory: PathBuf,
    objects: Vec<PathBuf>,
    host_exports: BTreeSet<String>,
    bootstrap: Option<PathBuf>,
}

impl NativeImageInputs {
    pub fn new(directory: PathBuf, objects: Vec<PathBuf>, host_exports: BTreeSet<String>) -> Self {
        Self { directory, objects, host_exports, bootstrap: None }
    }
    pub fn with_bootstrap(mut self, bootstrap: PathBuf) -> Self { self.bootstrap = Some(bootstrap); self }
    pub fn catalog(&self) -> PathBuf { self.directory.join(CATALOG_FILENAME) }
    pub fn required_exports(&self) -> PathBuf { self.directory.join(REQUIRED_EXPORTS_FILENAME) }
    pub fn objects(&self) -> impl Iterator<Item = &Path> { self.objects.iter().map(PathBuf::as_path) }
}
