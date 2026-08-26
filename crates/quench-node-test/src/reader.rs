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
    pub argv: Vec<String>,
}

impl NodeFixture {
    pub fn from_path(path: PathBuf) -> Result<Self, String> {
        let source =
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let path = path.canonicalize().unwrap_or(path);
        Ok(Self {
            path,
            source,
            argv: Vec::new(),
        })
    }

    pub fn from_source(path: PathBuf, source: String) -> Self {
        Self {
            path,
            source,
            argv: Vec::new(),
        }
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
        let (host, context) = quench_node::host::install_script_with_args(
            RealmId::ROOT,
            self.sink.clone(),
            &script,
            &fixture.argv,
        );
        self.host = host;
        self.context = context;
        if let Some(dir) = fixture.path.parent() {
            self.host.set_main_dir(dir.to_string_lossy().into_owned());
        }
        self.host
            .state()
            .borrow_mut()
            .process
            .unhandled_rejection_mode = rejection_mode(&fixture.source);
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
        let dgram_surface = if is_module && fixture_source.contains("dgram") {
            ["dgram-head", "dgram", "dgram-tail", "membership"]
                .into_iter()
                .filter_map(|name| quench_node::polyfills::bootstrap::lookup(name))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        };
        let url_pattern_surface =
            quench_node::polyfills::post_bootstrap::lookup("module-surface-06").unwrap_or("");
        let source = format!(
            "globalThis.URL = URL; Object.defineProperty(globalThis, '__nodeURL', {{ value: globalThis.URL, configurable: true }}); Object.defineProperty(globalThis, '__nodeURLSearchParams', {{ value: globalThis.URLSearchParams, configurable: true }});\n{url_pattern_surface}\nObject.defineProperty(globalThis, '__quenchURLPattern', {{ value: globalThis.__quenchURLPatternFactory?.(), configurable: true }}); delete globalThis.__quenchURLPatternFactory; delete globalThis.__quenchURLInstallCanParse; delete globalThis.__quenchURLInstallToString; delete globalThis.__nodeThrowReadonlyURLSetter;\nif (typeof globalThis.DOMException !== 'function') {{ Object.defineProperty(globalThis, 'DOMException', {{ configurable: true, enumerable: false, writable: true, value: class DOMException extends Error {{ constructor(message = '', name = 'Error') {{ super(message); this.name = name; this.code = {{ DataCloneError: 25, AbortError: 20 }}[name] || 0; }} }} }}); }}\n{source}"
        );
        let source = format!("{dgram_surface}\n{source}");
        let program = match reduce_fixture(&source, is_module) {
            Ok(program) => program,
            Err(error) => {
                return NodeOutcome::Fail {
                    reason: format!("reduce: {error}"),
                };
            }
        };
        let result = normalize_script_completion(
            quench_runtime::vm::execute_code_with_context(program.code(), &self.context),
        )
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
        match quench_runtime::vm::execute_code_with_context(program.code(), &self.context) {
            // Harness driver snippets are statements; normal completion has
            // no observable value and is represented by MissingReturn.
            Err(quench_runtime::vm::VmError::MissingReturn) => {
                Ok(quench_runtime::value::Value::Undefined)
            }
            result => result,
        }
    }
}

fn normalize_script_completion(
    result: Result<quench_runtime::value::Value, quench_runtime::vm::VmError>,
) -> Result<quench_runtime::value::Value, quench_runtime::vm::VmError> {
    match result {
        Err(quench_runtime::vm::VmError::MissingReturn) => {
            Ok(quench_runtime::value::Value::Undefined)
        }
        result => result,
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

fn rejection_mode(source: &str) -> quench_node::modules::process::UnhandledRejectionMode {
    let mode = source
        .lines()
        .find_map(|line| line.trim().strip_prefix("// Flags:"))
        .and_then(|flags| {
            flags
                .split_whitespace()
                .find_map(|flag| flag.strip_prefix("--unhandled-rejections="))
        });
    match mode {
        Some("none") => quench_node::modules::process::UnhandledRejectionMode::None,
        Some("warn") => quench_node::modules::process::UnhandledRejectionMode::Warn,
        Some("strict") => quench_node::modules::process::UnhandledRejectionMode::Strict,
        _ => quench_node::modules::process::UnhandledRejectionMode::Throw,
    }
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
        let source =
            "// Flags: --allow-natives-syntax\neval('%PrepareFunctionForOptimization(f)');\nf();\n";
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
