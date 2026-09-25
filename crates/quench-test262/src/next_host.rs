//! Minimal Test262 adapter for the staged v2 runtime.
//!
//! This adapter is intentionally separate from the legacy host while the
//! next runtime grows module, realm, and `$262` capability support. Keeping
//! the boundary explicit prevents legacy pass counts from being reported as
//! next-runtime evidence.

use std::path::Path;

use rqj::{
    CapabilityId, Engine, ExecutionRequest, Host, HostGlobal, ModuleSource, Runtime, SourceKind,
    SystemHost,
};

use crate::Test262Host;

static HOST_GLOBALS: [HostGlobal; 1] = [HostGlobal {
    name: "$262",
    capability: CapabilityId::CreateRealm,
}];
static ASYNC_GLOBALS: [HostGlobal; 2] = [
    HOST_GLOBALS[0],
    HostGlobal {
        name: "$DONE",
        capability: CapabilityId::Done,
    },
];

#[derive(Debug, Default)]
pub struct RuntimeNextHost {
    async_test: bool,
    can_block: bool,
    done: Option<String>,
}

impl Host for RuntimeNextHost {
    fn write_line(&mut self, text: &str) {
        if let Some(error) = text.strip_prefix("Test262:AsyncTestFailure:") {
            self.done = Some(error.to_string());
            return;
        }
        if text == "Test262:AsyncTestComplete" {
            self.done = Some(String::new());
            return;
        }
        println!("{text}");
    }

    fn clock_millis(&mut self) -> f64 {
        SystemHost.clock_millis()
    }

    fn globals(&self) -> &'static [HostGlobal] {
        if self.async_test {
            &ASYNC_GLOBALS
        } else {
            &HOST_GLOBALS
        }
    }

    fn done(&mut self, text: Option<&str>) {
        self.done = Some(text.unwrap_or_default().to_string());
    }

    fn can_block(&self) -> bool {
        self.can_block
    }

    fn resolve_dynamic_import(
        &mut self,
        referrer: &str,
        specifier: &str,
    ) -> Result<Option<ModuleSource>, String> {
        if specifier == "<module source>" {
            return Ok(Some(ModuleSource {
                name: specifier.to_string(),
                source: String::new(),
                bytes: Vec::new(),
            }));
        }
        if !specifier.starts_with('.') {
            return Ok(None);
        }
        let referrer = Path::new(referrer);
        let path = referrer
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(specifier);
        let bytes =
            std::fs::read(&path).map_err(|error| format!("module {}: {error}", path.display()))?;
        let source = String::from_utf8_lossy(&bytes).into_owned();
        Ok(Some(ModuleSource {
            name: path.display().to_string(),
            source,
            bytes,
        }))
    }
}

impl RuntimeNextHost {
    fn execute(&mut self, source: &str, name: &str) -> Result<(), String> {
        self.execute_kind(source, name, SourceKind::Script)
    }

    fn execute_kind(&mut self, source: &str, name: &str, kind: SourceKind) -> Result<(), String> {
        self.done = None;
        let async_test = self.async_test;
        let mut runtime = Runtime::new(std::mem::take(self));
        let result = (|| {
            let program = match kind {
                SourceKind::Script => Engine::specialize_unspecialized(source, name),
                SourceKind::Module => Engine::specialize_module_unspecialized(source, name),
                SourceKind::Eval => Engine::compile(ExecutionRequest { source, name, kind }),
            }
            .map_err(|errors| format!("next runtime SyntaxError: {errors:?}"))?;
            runtime.execute(&program).map_err(|error| {
                format!("next runtime: {}", runtime.format_error(&program, &error))
            })?;
            if async_test {
                runtime.run_jobs(&program).map_err(|error| {
                    format!(
                        "next runtime jobs: {}",
                        runtime.format_error(&program, &error)
                    )
                })?;
                if let Some(error) = runtime.host_mut().done.clone() {
                    if !error.is_empty() {
                        return Err(format!("next runtime async: {error}"));
                    }
                } else {
                    return Err("next runtime async: $DONE was not called".into());
                }
            }
            Ok(())
        })();
        *self = runtime.into_host();
        result
    }

    fn compose(harness: &[&str], source: &str, strict: bool) -> String {
        let mut composed = String::new();
        if strict {
            composed.push_str("\"use strict\";\n");
        }
        if source.starts_with("#!") {
            composed.push_str(source);
            composed.push('\n');
        }
        for script in harness {
            composed.push_str(script);
            composed.push('\n');
        }
        if !source.starts_with("#!") {
            composed.push_str(source);
        }
        composed
    }
}

impl Test262Host for RuntimeNextHost {
    fn configure(&mut self, metadata: &crate::TestMetadata) {
        self.async_test = metadata.is_async;
        self.can_block = metadata.can_block;
    }

    fn run_script(&mut self, source: &str) -> Result<(), String> {
        self.execute(source, "<test262>")
    }

    fn run_module_script(&mut self, source: &str) -> Result<(), String> {
        self.execute_kind(source, "<test262-module>", SourceKind::Module)
    }

    fn run_harnessed_script(
        &mut self,
        harness: &[&str],
        source: &str,
        strict: bool,
    ) -> Result<(), String> {
        self.execute(&Self::compose(harness, source, strict), "<test262-harness>")
    }

    fn run_harnessed_script_at(
        &mut self,
        harness: &[&str],
        source: &str,
        strict: bool,
        path: &Path,
    ) -> Result<(), String> {
        self.execute(
            &Self::compose(harness, source, strict),
            &path.display().to_string(),
        )
    }

    fn run_harnessed_module(&mut self, harness: &[&str], source: &str) -> Result<(), String> {
        self.execute_kind(
            &Self::compose(harness, source, false),
            "<test262-module>",
            SourceKind::Module,
        )
    }

    fn run_harnessed_module_at(
        &mut self,
        harness: &[&str],
        source: &str,
        path: &Path,
    ) -> Result<(), String> {
        self.execute_kind(
            &Self::compose(harness, source, false),
            &path.display().to_string(),
            SourceKind::Module,
        )
    }
}
