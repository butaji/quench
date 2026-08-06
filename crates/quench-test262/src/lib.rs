//! Test262 harness and runner boundary for Quench.
//!
//! This crate is the public home for conformance infrastructure. Runtime
//! users depend only on `quench-runtime`; Test262 consumers depend on this
//! crate for harness loading, metadata, host integration, and runners.

pub use quench_runtime::test262::*;

#[cfg(test)]
mod tests {
    use super::HarnessLoader;

    #[test]
    fn test262_boundary_exposes_harness_loader() {
        let loader = HarnessLoader::new("tests/test262");
        assert!(loader.root_dir().ends_with("tests/test262"));
    }
}
