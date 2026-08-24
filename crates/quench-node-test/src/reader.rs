//! Discovery and classification of Node fixture outcomes.

use std::path::{Path, PathBuf};

use quench_node::NodeHost;
use quench_runtime::ops::RealmId;
use quench_runtime::vm::VmContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeOutcome {
    Pass,
    Fail { reason: String },
    Skip { reason: String },
}

/// One Node fixture file + its raw source.
#[derive(Debug, Clone)]
pub struct NodeFixture {
    pub path: PathBuf,
    pub source: String,
}

impl NodeFixture {
    pub fn from_path(path: PathBuf) -> Result<Self, String> {
        let source =
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let path = path.canonicalize().unwrap_or(path);
        Ok(Self { path, source })
    }

    pub fn from_source(path: PathBuf, source: String) -> Self {
        Self { path, source }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn source(&self) -> &str {
        &self.source
    }
}

/// Canonical Node host runner. Owns the host handle and the
/// `VmContext`; both live for the duration of the run.
pub struct NodeRunner {
    pub host: std::rc::Rc<NodeHost>,
    pub context: VmContext,
    sink: std::sync::Arc<dyn Fn(&str) + Send + Sync>,
}

impl Default for NodeRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeRunner {
    pub fn new() -> Self {
        let (host, context) = quench_node::host::install(RealmId::ROOT);
        Self {
            host,
            context,
            sink: std::sync::Arc::new(|_| {}),
        }
    }

    pub fn with_output_sink(self, sink: std::sync::Arc<dyn Fn(&str) + Send + Sync>) -> Self {
        let (host, context) = quench_node::host::install_with_sink(RealmId::ROOT, sink.clone());
        Self {
            host,
            context,
            sink,
        }
    }

    /// Run one fixture and classify the completion.
    pub fn run(&mut self, fixture: &NodeFixture) -> NodeOutcome {
        // Fresh host per fixture with `node <file>` argv semantics.
        let script = fixture.path.to_string_lossy().into_owned();
        let (host, context) =
            quench_node::host::install_script(RealmId::ROOT, self.sink.clone(), &script);
        self.host = host;
        self.context = context;
        if let Some(dir) = fixture.path.parent() {
            self.host.set_main_dir(dir.to_string_lossy().into_owned());
        }
        let is_module = fixture
            .path
            .extension()
            .is_some_and(|extension| extension == "mjs");
        // CJS fixtures receive Node's wrapper; ESM fixtures retain module
        // syntax so the runtime's module reducer owns import facts.
        let fixture_source = strip_v8_native_probes(&fixture.source);
        let source = if is_module {
            quench_node::esm_imports::transform_esm_imports(&fixture_source)
        } else {
            quench_node::modules::require::wrap_cjs(&self.host.state(), &script, &fixture_source)
        };
        let source = if fixture.source.contains("--experimental-eventsource") {
            format!("globalThis.EventSource = globalThis.__quench_event_source;\n{source}")
        } else {
            source
        };
        let program = match reduce_fixture(&source, is_module) {
            Ok(program) => program,
            Err(error) => {
                return NodeOutcome::Fail {
                    reason: format!("reduce: {error}"),
                };
            }
        };
        let result = quench_runtime::vm::execute_code_with_context(program.code(), &self.context)
            .and_then(|_| self.drive("__quench_run_loop__();"))
            .and_then(|_| {
                self.drive(
                    "if (typeof globalThis.__quench_verify_calls__ === 'function') globalThis.__quench_verify_calls__();",
                )
            });
        let result = self.route_uncaught(result);
        // `process.exit` unwinds with an error; `exit` handlers still run.
        let result = match result {
            Err(error) => match self.drive("__quench_run_exit__();") {
                Ok(_) => Err(error),
                Err(exit_error) => Err(exit_error),
            },
            ok => ok.map(|_| ()),
        };
        Self::classify(result, self.host.exit_code())
    }

    /// Node dispatches top-level uncaught exceptions to
    /// `process.on('uncaughtException')`; a handled run continues.
    fn route_uncaught(
        &self,
        result: Result<quench_runtime::value::Value, quench_runtime::vm::VmError>,
    ) -> Result<(), quench_runtime::vm::VmError> {
        match result {
            Err(error) => {
                match quench_node::modules::pump::handle_uncaught(&self.host.state(), error) {
                    Ok(()) => self
                        .drive("__quench_uncaught__();")
                        .and_then(|_| self.drive("__quench_run_loop__();"))
                        .map(|_| ()),
                    Err(error) => Err(error),
                }
            }
            ok => ok.map(|_| ()),
        }
    }

    fn classify(
        result: Result<(), quench_runtime::vm::VmError>,
        exit_code: Option<i32>,
    ) -> NodeOutcome {
        match (result, exit_code) {
            (Ok(_), None | Some(0)) => NodeOutcome::Pass,
            (Ok(_), Some(code)) => NodeOutcome::Fail {
                reason: format!("exit code {code}"),
            },
            (Err(_), Some(0)) => NodeOutcome::Pass,
            (Err(error), Some(code)) => NodeOutcome::Fail {
                reason: format!("exit code {code}: {}", error.render()),
            },
            (Err(error), None) => NodeOutcome::Fail {
                reason: format!("runtime: {error:?}"),
            },
        }
    }

    /// Execute a tiny driver snippet (e.g. `__quench_run_loop__();`)
    /// in the same context, so the host pump runs inside an active
    /// execution frame with globals and capabilities available.
    fn drive(
        &self,
        source: &str,
    ) -> Result<quench_runtime::value::Value, quench_runtime::vm::VmError> {
        let program =
            reduce_fixture(source, false).map_err(quench_runtime::vm::VmError::EvalError)?;
        quench_runtime::vm::execute_code_with_context(program.code(), &self.context)
    }
}

/// V8 optimization intrinsics are test-harness probes, not Node API behavior.
/// Quench has no V8 optimizing tier, so remove only those eval statements when
/// the upstream fixture explicitly requests `--allow-natives-syntax`.
fn strip_v8_native_probes(source: &str) -> String {
    if !source.contains("--allow-natives-syntax") {
        return source.to_string();
    }
    source
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !(trimmed.starts_with("eval('%") && trimmed.ends_with("');"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn reduce_fixture(
    source: &str,
    _is_module: bool,
) -> Result<quench_runtime::reduce::reduce_statements::ResidualProgram, String> {
    let result = quench_runtime::reduce::reduce_source(source);
    let program = result.map_err(|errors| errors.join("; "))?;
    Ok(program)
}

#[cfg(test)]
mod tests {
    use super::strip_v8_native_probes;

    #[test]
    fn strips_only_requested_v8_probe_statements() {
        let source = "// Flags: --allow-natives-syntax\neval('%PrepareFunctionForOptimization(f)');\nf();\n";
        let normalized = strip_v8_native_probes(source);
        assert!(!normalized.contains("PrepareFunctionForOptimization"));
        assert!(normalized.contains("f();"));
    }

    #[test]
    fn keeps_native_probe_without_capability_flag() {
        let source = "eval('%PrepareFunctionForOptimization(f)');\n";
        assert_eq!(strip_v8_native_probes(source), source);
    }
}
