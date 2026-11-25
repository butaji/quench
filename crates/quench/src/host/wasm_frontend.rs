//! The Node host's WebAssembly frontend, installed on a host whose program can reach
//! `WebAssembly` and left out of one that cannot: the functions live in this module, so a host
//! that never names them links no decoder.

use quench_runtime::{ExecutionMode, WasmModule};

/// The frontend an image's catalog installs when its program reads `WebAssembly`.
pub const FRONTEND: quench_runtime::WasmFrontend = quench_runtime::WasmFrontend {
    validate,
    compile,
};

/// Validate a binary without producing executable code.
pub(crate) fn validate(bytes: &[u8], _mode: ExecutionMode) -> bool {
    quench_wasm::validate_binary(bytes)
}

/// Decode, validate and lower a binary for `mode`. Decoding, lowering and the phases around
/// them each touch code the others do not, so each phase's pages leave the resident set before
/// the next phase runs the module.
pub(crate) fn compile(bytes: &[u8], mode: ExecutionMode) -> Result<WasmModule, String> {
    crate::host::resident_pages::release_read_only_pages();
    let module = quench_wasm::Engine::new().compile(bytes);
    crate::host::resident_pages::release_read_only_pages();
    let module = module
        .and_then(|module| module.lower_shared_module_for(mode))
        .map_err(|error| error.to_string());
    crate::host::resident_pages::release_read_only_pages();
    module
}
