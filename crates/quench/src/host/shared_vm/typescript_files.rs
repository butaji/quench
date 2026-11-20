//! Node's TypeScript file extensions and the module goal each one fixes (type stripping):
//! `.ts` follows its package scope like `.js`, `.mts` is always ESM, `.cts` always CommonJS.
//! Node runs no `.tsx`. Every site that asks "is this TypeScript?" asks here.
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TypeScriptFile {
    /// `.ts`: the package scope decides, as for `.js`.
    PackageScoped,
    /// `.mts`.
    Module,
    /// `.cts`.
    CommonJs,
}

impl TypeScriptFile {
    pub(crate) fn of(path: &Path) -> Option<Self> {
        Self::of_extension(path.extension()?.to_str()?)
    }

    pub(crate) fn of_extension(extension: &str) -> Option<Self> {
        match extension {
            "ts" => Some(Self::PackageScoped),
            "mts" => Some(Self::Module),
            "cts" => Some(Self::CommonJs),
            _ => None,
        }
    }

    /// A file `require` may load as CommonJS: `.ts` or `.cts`.
    pub(crate) fn loads_as_commonjs(self) -> bool {
        self != Self::Module
    }

    /// A file the ESM loader may load as a module: `.ts` or `.mts`.
    pub(crate) fn loads_as_module(self) -> bool {
        self != Self::CommonJs
    }
}
