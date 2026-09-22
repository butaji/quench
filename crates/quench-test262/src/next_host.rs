//! Minimal Test262 adapter for the staged v2 runtime.
//!
//! This adapter is intentionally separate from the legacy host while the
//! next runtime grows module, realm, and `$262` capability support. Keeping
//! the boundary explicit prevents legacy pass counts from being reported as
//! next-runtime evidence.

use std::path::Path;

use rqj::{Engine, Runtime, SystemHost};

use crate::Test262Host;

#[derive(Debug, Default)]
pub struct RuntimeNextHost;

impl RuntimeNextHost {
    fn execute(&mut self, source: &str, name: &str) -> Result<(), String> {
        let mut runtime = Runtime::new(SystemHost);
        let program = Engine::specialize_unspecialized(source, name)
            .map_err(|errors| format!("next runtime diagnostics: {errors:?}"))?;
        runtime
            .execute(&program)
            .map(|_| ())
            .map_err(|error| format!("next runtime: {error:?}"))
    }

    fn compose(harness: &[&str], source: &str, strict: bool) -> String {
        let mut composed = String::new();
        if strict {
            composed.push_str("\"use strict\";\n");
        }
        for script in harness {
            composed.push_str(script);
            composed.push('\n');
        }
        composed.push_str(source);
        composed
    }
}

impl Test262Host for RuntimeNextHost {
    fn run_script(&mut self, source: &str) -> Result<(), String> {
        self.execute(source, "<test262>")
    }

    fn run_module_script(&mut self, _source: &str) -> Result<(), String> {
        Err("next runtime: module execution is not available yet".into())
    }

    fn run_harnessed_script(
        &mut self,
        harness: &[&str],
        source: &str,
        strict: bool,
    ) -> Result<(), String> {
        self.execute(&Self::compose(harness, source, strict), "<test262-harness>")
    }

    fn run_harnessed_module(&mut self, _harness: &[&str], _source: &str) -> Result<(), String> {
        Err("next runtime: module execution is not available yet".into())
    }

    fn run_harnessed_module_at(
        &mut self,
        _harness: &[&str],
        _source: &str,
        _path: &Path,
    ) -> Result<(), String> {
        Err("next runtime: module execution is not available yet".into())
    }
}
