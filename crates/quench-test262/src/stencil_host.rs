//! Test262 adapter for the migrated stencil VM.
//!
//! This host deliberately contains no conformance policy. The runner composes
//! harness and test sources, then this adapter submits the resulting source to
//! `quench-runtime::vm_core` so the same core used by Node file execution is
//! exercised by the conformance corpus.

use std::path::Path;

use crate::{Test262Host, TestMetadata};

#[derive(Debug, Default)]
pub struct StencilHost {}

impl StencilHost {
    fn run_source_at(&self, source: &str, path: &Path) -> Result<(), String> {
        let argv = vec!["quench-node".to_string(), path.display().to_string()];
        quench_runtime::vm_core::run_source_with_argv_and_output_status(
            path,
            source,
            argv,
            Vec::new(),
            |_| {},
        )
        .map(|_| ())
    }

    fn compose(harness: &[&str], source: &str, strict: bool) -> String {
        let mut composed = String::new();
        if strict {
            composed.push_str("\"use strict\";\n");
        }
        for unit in harness {
            composed.push_str(unit);
            composed.push('\n');
        }
        composed.push_str(source);
        composed
    }
}

impl Test262Host for StencilHost {
    fn configure(&mut self, metadata: &TestMetadata) {
        let _ = metadata;
    }

    fn run_script(&mut self, source: &str) -> Result<(), String> {
        self.run_source_at(source, Path::new("<test262>.cjs"))
    }

    fn run_module_script(&mut self, source: &str) -> Result<(), String> {
        self.run_source_at(source, Path::new("<test262-module>.mjs"))
    }

    fn run_harnessed_script(
        &mut self,
        harness: &[&str],
        source: &str,
        strict: bool,
    ) -> Result<(), String> {
        self.run_source_at(
            &Self::compose(harness, source, strict),
            Path::new("<test262-harnessed>.cjs"),
        )
    }

    fn run_harnessed_module(&mut self, harness: &[&str], source: &str) -> Result<(), String> {
        self.run_source_at(
            &Self::compose(harness, source, false),
            Path::new("<test262-module-harnessed>.mjs"),
        )
    }

    fn run_harnessed_module_at(
        &mut self,
        harness: &[&str],
        source: &str,
        path: &Path,
    ) -> Result<(), String> {
        // The path-aware runner receives Test262's `.js` fixture path even
        // for files flagged `module`.  The shared core derives OXC's
        // SourceType from the path, so preserve the module grammar explicitly
        // at this boundary while retaining the original basename/location.
        let module_path = path.with_extension("mjs");
        self.run_source_at(&Self::compose(harness, source, false), &module_path)
    }
}

/// Select the conformance host from `QUENCH_TEST262_ENGINE`.
///
/// The stencil core is the only supported execution path. The environment
/// variable is retained as an explicit compatibility diagnostic switch while
/// downstream callers finish migrating; ordinary runs never silently select
/// the residual VM host.
pub fn selected_host() -> Box<dyn Test262Host> {
    match std::env::var("QUENCH_TEST262_ENGINE").as_deref() {
        Ok("compat") => Box::new(crate::RuntimeHost),
        _ => Box::new(StencilHost::default()),
    }
}
