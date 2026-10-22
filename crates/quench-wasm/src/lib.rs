//! WebAssembly format: parse, validate and lower for the shared VM.
//!
//! This crate owns the binary and text formats. Quench's shared VM owns execution; the
//! spec-suite (WAST) harness lives in quench-wasm-test, so builds of quench-node do not
//! compile the harness host's copy of the VM.

use std::fmt;

mod decode;
mod interface;
pub use interface::{interface, Export, ExternKind, Import, Interface};
mod shared;
pub use shared::decode_globals;

pub use decode::{
    core_features, features_for_path, inspect_binary, inspect_binary_with, ModuleStatus,
};

/// Errors at the Wasm frontend boundary.
#[derive(Debug)]
pub enum Error {
    Parse(String),
    Validate(String),
    Unsupported(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(message) => write!(f, "parse error: {message}"),
            Self::Validate(message) => write!(f, "validate error: {message}"),
            Self::Unsupported(message) => write!(f, "unsupported shared Wasm: {message}"),
        }
    }
}

impl std::error::Error for Error {}

/// Wasm frontend. Compile is parse plus validate; it does not instantiate.
#[derive(Clone, Copy, Debug, Default)]
pub struct Engine;

impl Engine {
    pub fn new() -> Self {
        Self
    }

    /// Parse and validate binary Wasm with core features. Does not instantiate.
    pub fn compile(&self, bytes: &[u8]) -> Result<Module, Error> {
        match inspect_binary(bytes) {
            ModuleStatus::Valid => Ok(Module {
                bytes: bytes.to_vec(),
            }),
            ModuleStatus::ParseError(message) => Err(Error::Parse(message)),
            ModuleStatus::ValidateError(message) => Err(Error::Validate(message)),
        }
    }

    pub fn compile_wat(&self, source: &str) -> Result<Module, Error> {
        self.compile_wat_with_features(source, core_features())
    }

    pub fn compile_wat_with_features(
        &self,
        source: &str,
        features: wasmparser::WasmFeatures,
    ) -> Result<Module, Error> {
        let buf =
            wast::parser::ParseBuffer::new(source).map_err(|e| Error::Parse(e.to_string()))?;
        let mut wat: wast::Wat<'_> =
            wast::parser::parse(&buf).map_err(|e| Error::Parse(e.to_string()))?;
        let bytes = wat.encode().map_err(|e| Error::Parse(e.to_string()))?;
        match inspect_binary_with(&bytes, features) {
            ModuleStatus::Valid => Ok(Module { bytes }),
            ModuleStatus::ParseError(message) => Err(Error::Parse(message)),
            ModuleStatus::ValidateError(message) => Err(Error::Validate(message)),
        }
    }
}

/// A parsed and validated module. Instantiation is not implemented in this phase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    bytes: Vec<u8>,
}

impl Module {
    /// A module of `bytes` its caller already parsed and validated under the features it
    /// runs with (the WAST harness validates each directive's module itself).
    pub fn from_validated(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::Engine;

    #[test]
    fn compile_validates_without_instantiating() {
        let module = Engine::new()
            .compile_wat("(module (func (export \"add\") (param i32 i32) (result i32) local.get 0 local.get 1 i32.add))")
            .expect("compile");
        assert!(module.bytes().starts_with(b"\0asm"));
    }

    #[test]
    fn compile_rejects_invalid() {
        let error = Engine::new()
            .compile_wat("(module (func (unreachable) (drop (local.get 0))))")
            .expect_err("invalid");
        assert!(matches!(error, super::Error::Validate(_)), "{error:?}");
    }
}
