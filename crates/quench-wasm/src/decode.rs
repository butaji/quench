//! Third-party binary parse vs validate, with spec-suite per-path features.

use std::path::Path;

use wasmparser::{Parser, Validator, WasmFeatures};

/// Outcome of inspecting a Wasm binary without executing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleStatus {
    /// The bytes are not a well-formed module (decode/parse failed).
    ParseError(String),
    /// The bytes decoded, then failed type validation.
    ValidateError(String),
    /// Decode and validation both succeeded.
    Valid,
}

/// Wasm 3.0 core minus extra proposals the amalgamated suite tests *without*.
///
/// `WasmFeatures::all()` / `WASM3` with threads+stack-switching+descriptors
/// enabled makes modules the suite marks invalid validate successfully.
pub fn core_features() -> WasmFeatures {
    WasmFeatures::WASM2
        | WasmFeatures::GC
        | WasmFeatures::TAIL_CALL
        | WasmFeatures::EXTENDED_CONST
        | WasmFeatures::FUNCTION_REFERENCES
        | WasmFeatures::MULTI_MEMORY
        | WasmFeatures::RELAXED_SIMD
        | WasmFeatures::EXCEPTIONS
        | WasmFeatures::MEMORY64
}

/// What the JavaScript API (`WebAssembly.validate`, `compile`, `Module`) accepts: the core set
/// and the legacy exception-handling proposal (`try`, `catch`, `catch_all`, `delegate`,
/// `rethrow`), which V8 and JavaScriptCore still ship and toolchains such as .NET's still
/// emit (Node v26.10.0 validates such a module). The official suite tests core without it.
pub fn js_api_features() -> WasmFeatures {
    core_features() | WasmFeatures::LEGACY_EXCEPTIONS
}

/// Features the official suite uses for a `.wast` path.
pub fn features_for_path(path: impl AsRef<Path>) -> WasmFeatures {
    let path = path.as_ref();
    let text = path.to_string_lossy().replace('\\', "/");
    match proposal_from_path(&text) {
        Some("threads") => (WasmFeatures::WASM2 | WasmFeatures::THREADS)
            .difference(WasmFeatures::REFERENCE_TYPES)
            .difference(WasmFeatures::MULTI_MEMORY),
        Some("custom-page-sizes") => {
            core_features() | WasmFeatures::CUSTOM_PAGE_SIZES | WasmFeatures::MEMORY64
        }
        Some("custom-descriptors") => core_features() | WasmFeatures::CUSTOM_DESCRIPTORS,
        Some("wide-arithmetic") => core_features() | WasmFeatures::WIDE_ARITHMETIC,
        Some(_) => core_features(),
        None if text.contains("/legacy/") => core_features() | WasmFeatures::LEGACY_EXCEPTIONS,
        None => core_features(),
    }
}

fn proposal_from_path(path: &str) -> Option<&str> {
    let mut parts = path.split('/');
    while let Some(part) = parts.next() {
        if part == "proposals" {
            return parts.next();
        }
    }
    None
}

/// Decode `bytes` then validate with `features`.
pub fn inspect_binary_with(bytes: &[u8], features: WasmFeatures) -> ModuleStatus {
    let mut parser = Parser::new(0);
    parser.set_features(features);
    for payload in parser.parse_all(bytes) {
        if let Err(error) = payload {
            return ModuleStatus::ParseError(error.to_string());
        }
    }
    let mut validator = Validator::new_with_features(features);
    match validator.validate_all(bytes) {
        Ok(_) => ModuleStatus::Valid,
        Err(error) => ModuleStatus::ValidateError(error.to_string()),
    }
}

/// Decode and validate with Wasm 3.0 core features.
pub fn inspect_binary(bytes: &[u8]) -> ModuleStatus {
    inspect_binary_with(bytes, js_api_features())
}

