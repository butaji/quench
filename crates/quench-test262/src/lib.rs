//! Test262 harness and runner boundary for Quench.
//!
//! This crate is the public home for conformance infrastructure. Runtime
//! users depend only on `quench-runtime`; Test262 consumers depend on this
//! crate for harness loading, metadata, host integration, and runners.

pub mod harness {
    pub use super::test262::harness::*;
}
pub mod host {
    pub use super::test262::host::*;
}
pub mod metadata {
    pub use super::test262::metadata::*;
}
pub mod runner {
    pub use super::test262::runner::*;
    pub use super::test262::runner::execute;
}
pub mod skip {
    pub use super::test262::skip::*;
}

mod test262;

pub use test262::*;
pub use quench_runtime::{DefaultQuenchRuntime, HostCallback, JsResult, QuenchRuntime};

#[cfg(test)]
mod tests {
    use super::{DefaultQuenchRuntime, HarnessLoader, QuenchRuntime};

    #[test]
    fn test262_boundary_exposes_harness_loader() {
        let loader = HarnessLoader::new("tests/test262");
        assert!(loader.root_dir().ends_with("tests/test262"));
    }

    #[test]
    fn test262_boundary_uses_runtime_api() {
        let mut engine = DefaultQuenchRuntime;
        let mut ctx = engine.new_context().unwrap();
        assert_eq!(engine.eval(&mut ctx, "6 * 7").unwrap().to_string(), "42");
    }
}
