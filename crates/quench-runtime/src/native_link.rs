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
    /// No dynamic interface: nothing the image runs loads native code, so it links its C
    /// runtime statically and exports nothing (`NativeImageInputs::self_contained`).
    self_contained: bool,
}

impl NativeImageInputs {
    pub fn new(directory: PathBuf, objects: Vec<PathBuf>, host_exports: BTreeSet<String>) -> Self {
        Self { directory, objects, host_exports, bootstrap: None, self_contained: false }
    }
    /// An image whose program loads no native addon: it needs no host exports (Node-API's are
    /// what keep that implementation linked) and no dynamic loader, so on Linux it links its C
    /// runtime statically (no `ld.so` relocation at start, no shared libc pages).
    pub fn self_contained(mut self) -> Self { self.host_exports.clear(); self.self_contained = true; self }
    pub fn is_self_contained(&self) -> bool { self.self_contained }
    pub fn with_bootstrap(mut self, bootstrap: PathBuf) -> Self { self.bootstrap = Some(bootstrap); self }
    pub fn catalog(&self) -> PathBuf { self.directory.join(CATALOG_FILENAME) }
    pub fn required_exports(&self) -> PathBuf { self.directory.join(REQUIRED_EXPORTS_FILENAME) }
    pub fn objects(&self) -> impl Iterator<Item = &Path> { self.objects.iter().map(PathBuf::as_path) }
}
