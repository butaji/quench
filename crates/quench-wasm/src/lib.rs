//! WebAssembly format: parse, validate, and wast scoring.
//!
//! This crate owns the binary/text format and the spec-suite harness.
//! `quench-runtime` is the VM: load, instantiate, interpret.

use std::fmt;

mod decode;
mod legacy_try;
mod wast_exec;
mod wast_script;

pub use decode::{
    core_features, features_for_path, inspect_binary, inspect_binary_with, ModuleStatus,
};
pub use wast_script::{run_wast, DirectiveResult, WastReport};

/// Errors at the Wasm frontend boundary.
#[derive(Debug)]
pub enum Error {
    Parse(String),
    Validate(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(message) => write!(f, "parse error: {message}"),
            Self::Validate(message) => write!(f, "validate error: {message}"),
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
        let buf =
            wast::parser::ParseBuffer::new(source).map_err(|e| Error::Parse(e.to_string()))?;
        let mut wat: wast::Wat<'_> =
            wast::parser::parse(&buf).map_err(|e| Error::Parse(e.to_string()))?;
        let bytes = wat.encode().map_err(|e| Error::Parse(e.to_string()))?;
        self.compile(&bytes)
    }

    /// Score every directive in a wast script using features implied by `filename`.
    pub fn run_wast(&self, filename: &str, source: &str) -> WastReport {
        run_wast(filename, source)
    }
}

/// A parsed and validated module. Instantiation is not implemented in this phase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    bytes: Vec<u8>,
}

impl Module {
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

    #[test]
    fn engine_run_wast_scores_directives() {
        let report = Engine::new().run_wast(
            "mix.wast",
            r#"
(assert_malformed (module binary "") "unexpected end")
(module (func (export "x") (result i32) i32.const 1))
(assert_return (invoke "x") (i32.const 1))
"#,
        );
        assert_eq!(report.results.len(), 3);
        assert!(report.results[0].passed);
        assert!(report.results[1].passed);
        assert!(report.results[2].passed, "{:?}", report.results[2]);
    }
}
