//! WebAssembly format: parse, validate and lower for the shared VM.
//!
//! This crate owns the binary and text formats. Quench's shared VM owns execution; the
//! spec-suite (WAST) harness lives in quench-wasm-test, so builds of quench-node do not
//! compile the harness host's copy of the VM.

use std::fmt;
use std::borrow::Cow;

mod decode;
mod interface;
pub use interface::{interface, Export, ExternKind, Import, Interface};
mod shared;
pub use shared::decode_globals;

pub use decode::{
    core_features, features_for_path, inspect_binary, inspect_binary_with, js_api_features, validate_binary, ModuleStatus,
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
    /// The module borrows `bytes`: lowering reads them in place, so a compile holds no second
    /// copy of the wire bytes beside its caller's (dotnet-aot-wasm: 7.8 MB).
    pub fn compile<'b>(&self, bytes: &'b [u8]) -> Result<Module<'b>, Error> {
        match inspect_binary(bytes) {
            ModuleStatus::Valid => Ok(Module {
                bytes: Cow::Borrowed(bytes),
            }),
            ModuleStatus::ParseError(message) => Err(Error::Parse(message)),
            ModuleStatus::ValidateError(message) => Err(Error::Validate(message)),
        }
    }

    pub fn compile_wat(&self, source: &str) -> Result<Module<'static>, Error> {
        self.compile_wat_with_features(source, core_features())
    }

    pub fn compile_wat_with_features(
        &self,
        source: &str,
        features: wasmparser::WasmFeatures,
    ) -> Result<Module<'static>, Error> {
        let buf =
            wast::parser::ParseBuffer::new(source).map_err(|e| Error::Parse(e.to_string()))?;
        let mut wat: wast::Wat<'_> =
            wast::parser::parse(&buf).map_err(|e| Error::Parse(e.to_string()))?;
        let bytes = wat.encode().map_err(|e| Error::Parse(e.to_string()))?;
        match inspect_binary_with(&bytes, features) {
            ModuleStatus::Valid => Ok(Module { bytes: Cow::Owned(bytes) }),
            ModuleStatus::ParseError(message) => Err(Error::Parse(message)),
            ModuleStatus::ValidateError(message) => Err(Error::Validate(message)),
        }
    }
}

/// A parsed and validated module, over its wire bytes, borrowed or owned. Instantiation is
/// not implemented in this phase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module<'b> {
    bytes: Cow<'b, [u8]>,
}

impl Module<'static> {
    /// A module of `bytes` its caller already parsed and validated under the features it
    /// runs with (the WAST harness validates each directive's module itself).
    pub fn from_validated(bytes: Vec<u8>) -> Self {
        Self { bytes: Cow::Owned(bytes) }
    }
}

impl Module<'_> {
    /// The module over its own copy of the bytes, for a caller that drops them first.
    pub fn into_owned(self) -> Module<'static> {
        Module { bytes: Cow::Owned(self.bytes.into_owned()) }
    }


    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::Engine;

    #[test]
    fn javascript_validation_accepts_threads_without_changing_the_core_profile() {
        let module = Engine::new().compile_wat_with_features(
            "(module (memory 1 1 shared))",
            super::core_features() | wasmparser::WasmFeatures::THREADS,
        ).unwrap();
        assert!(super::validate_binary(module.bytes()));
        assert!(matches!(super::inspect_binary(module.bytes()), super::ModuleStatus::Valid));
        assert!(!matches!(super::inspect_binary_with(module.bytes(), super::core_features()), super::ModuleStatus::Valid));
        assert!(module.lower_shared_module().is_ok());
        assert!(!super::validate_binary(b"invalid module"));
    }

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
