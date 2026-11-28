//! Host-neutral native executable publication, available only in compiler builds.

pub mod image_dump;
mod exports;
mod footprint;
mod link;
mod payload;
pub use exports::{DynamicInterface, ExportPolicy};
pub use link::{NativeLinkAudit, NativeSupport, checked_output};
pub use footprint::NativeImageFootprint;
pub use payload::{PayloadReduction, remove_offline_symbols};

/// The sections an image's marks live in, by platform (`shared_vm::marked_operation`).
const MARK_SECTIONS: [&str; 2] = [".quench_operation_marks", "__quench_marks"];

/// The little-endian 32-bit word of each symbol of `executable` whose name holds `marker`, or every word of the marks'
/// section where the image has one:
/// statics the linker kept because the code it kept refers to them.
pub fn marked_words(executable: &std::path::Path, marker: &str) -> Result<Vec<u32>, String> {
    use object::{Object, ObjectSection, ObjectSymbol};
    let bytes = std::fs::read(executable).map_err(|error| format!("{}: {error}", executable.display()))?;
    let file = object::File::parse(&*bytes).map_err(|error| error.to_string())?;
    // The marks' own section (`MARK_SECTIONS`), whole: every word in it is a mark.
    if let Some(section) = file.sections().find(|section| section.name().is_ok_and(|name| MARK_SECTIONS.contains(&name))) {
        let data = section.data().map_err(|error| error.to_string())?;
        return Ok(data.chunks_exact(4).map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]])).collect());
    }
    let mut words = Vec::new();
    for symbol in file.symbols().filter(|symbol| symbol.name().is_ok_and(|name| name.contains(marker))) {
        let Some(index) = symbol.section_index() else { continue };
        let section = file.section_by_index(index).map_err(|error| error.to_string())?;
        let data = section.data().map_err(|error| error.to_string())?;
        let offset = usize::try_from(symbol.address() - section.address()).map_err(|error| error.to_string())?;
        let word = data.get(offset..offset + 4).ok_or_else(|| format!("mark {} lies outside its section", symbol.name().unwrap_or("?")))?;
        words.push(u32::from_le_bytes([word[0], word[1], word[2], word[3]]));
    }
    Ok(words)
}

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
