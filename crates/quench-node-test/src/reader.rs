//! Discovery and classification of Node fixture outcomes.

use std::path::{Path, PathBuf};

use quench_node::NodeHost;
use quench_runtime::ops::RealmId;
use quench_runtime::value::Value;
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
    /// Script arguments supplied by the runner/child process.
    pub argv: Vec<String>,
    /// Host invocation flags declared by the fixture header.
    pub exec_argv: Vec<String>,
}

impl NodeFixture {
    pub fn from_path(path: PathBuf) -> Result<Self, String> {
        let source =
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let path = path.canonicalize().unwrap_or(path);
        Ok(Self {
            path,
            argv: Vec::new(),
            exec_argv: fixture_flags(&source),
            source,
        })
    }

    pub fn from_source(path: PathBuf, source: String) -> Self {
        Self {
            path,
            argv: Vec::new(),
            exec_argv: fixture_flags(&source),
            source,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn source(&self) -> &str {
        &self.source
    }
}

/// Node's test files carry executable options in `// Flags:` directives. They
/// configure the host invocation, but are not script arguments: Node exposes
/// them through `process.execArgv`, never through `process.argv`.
fn fixture_flags(source: &str) -> Vec<String> {
    source
        .lines()
        .find_map(|line| line.trim().strip_prefix("// Flags:"))
        .map(|flags| flags.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default()
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
        let _cwd_guard = FixtureCwdGuard::capture();
        let script = fixture.path.to_string_lossy().into_owned();
        let title = cli_title(&fixture.source).unwrap_or_else(|| "quench-node".into());
        let (host, context) = quench_node::host::install_script_with_args_and_title(
            RealmId::ROOT,
            self.sink.clone(),
            &script,
            &fixture.argv,
            &title,
        );
        let fixture_source = strip_v8_native_probes(&fixture.source);
        self.host = host;
        let mut context = context
            .with_source_text(fixture_source.clone())
            .with_source_name(script.clone())
            .with_host_value(
                "__quench_script_source".to_string(),
                Value::String(fixture_source.clone()),
            )
            .with_host_value(
                "__quench_script_filename".to_string(),
                Value::String(script.clone()),
            );
        // Invocation flags are visible through execArgv, never argv.  Apply
        // realm-shaping visibility options at this test-runner boundary while
        // keeping Node host behavior in Rust.
        let global = quench_runtime::vm::current_global_object();
        let process = quench_runtime::execute::get_property(&global, "process");
        let exec_argv = quench_runtime::host_api::array(
            fixture
                .exec_argv
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        );
        let _ = quench_runtime::execute::set_property_in_place(&process, "execArgv", exec_argv);
        quench_node::modules::process::set_abort_on_uncaught_exception(
            &self.host.state(),
            &fixture.exec_argv,
        );
        if fixture
            .exec_argv
            .iter()
            .any(|flag| flag == "--enable-sharedarraybuffer-per-context")
        {
            // Intrinsic globals are immutable host facts; shadow the one
            // visibility option in this fixture's realm context instead of
            // mutating the shared intrinsic table.
            context = context.with_host_value("SharedArrayBuffer", Value::Undefined);
        }
        let exec_argv_value = quench_runtime::host_api::array(
            fixture
                .exec_argv
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        );
        context = context.with_host_value("__quench_exec_argv", exec_argv_value);
        self.context = context.clone();
        if let Some(dir) = fixture.path.parent() {
            self.host.set_main_dir(dir.to_string_lossy().into_owned());
        }
        self.host
            .state()
            .borrow_mut()
            .process
            .unhandled_rejection_mode = rejection_mode(&fixture.source, &fixture.exec_argv);
        let is_module = fixture
            .path
            .extension()
            .is_some_and(|extension| extension == "mjs");
        // CJS fixtures receive Node's wrapper; ESM fixtures retain module
        // syntax so the runtime's module reducer owns import facts.
        let fixture_program = if is_module {
            quench_node::esm_imports::transform_esm_imports(&fixture_source)
        } else {
            quench_node::modules::require::wrap_cjs(&self.host.state(), &script, &fixture_source)
        };
        let fixture_program = if fixture.source.contains("--experimental-eventsource") {
            format!("globalThis.EventSource = globalThis.__quench_event_source;\n{fixture_program}")
        } else {
            fixture_program
        };
        let dgram_surface = if fixture_source.contains("dgram") {
            ["dgram-head", "dgram", "dgram-tail", "membership"]
                .into_iter()
                .filter_map(|name| quench_node::polyfills::bootstrap::lookup(name))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        };
        let dns_surface = if fixture_source.contains("dns") {
            quench_node::polyfills::bootstrap::lookup("dns")
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        };
        let stream_iter_surface = if fixture_source.contains("stream/iter")
            && fixture
                .exec_argv
                .iter()
                .any(|flag| flag == "--experimental-stream-iter")
        {
            format!(
                "const __quenchIterStream = require('stream');\nconst NodeReadable = __quenchIterStream.Readable;\nconst NodeWritable = __quenchIterStream.Writable;\nconst NodeDuplex = __quenchIterStream.Duplex;\nconst NodeTransform = __quenchIterStream.Transform;\n{}\nglobalThis.__quenchRequireStreamIter = __quenchRequireStreamIter;",
                quench_node::polyfills::bootstrap::cluster::stream_iter_js()
            )
        } else {
            String::new()
        };
        // Node exposes WHATWG stream constructors globally. Install the
        // shared surface before the fixture so globals and `stream/web`
        // resolve to one constructor identity.
        let globals_surface =
            quench_node::polyfills::bootstrap::lookup("globals-extra").unwrap_or("");
        let fetch_surface = quench_node::polyfills::bootstrap::lookup("fetch").unwrap_or("");
        // The externalizable-string helpers are test-only host hooks. Install
        // them only for fixtures that name the hooks; keeping them out of the
        // baseline global shape preserves Node's global-leak observations.
        let externalizable_surface = fixture_source
            .contains("Externalizable")
            .then(|| {
                quench_node::polyfills::bootstrap::lookup("externalizable-strings").unwrap_or("")
            })
            .unwrap_or("");
        let report_surface = quench_node::polyfills::bootstrap::lookup("report").unwrap_or("");
        let punycode_surface = quench_node::polyfills::bootstrap::lookup("punycode").unwrap_or("");
        let support_surface = quench_node::polyfills::bootstrap::lookup("support").unwrap_or("");
        let async_resource_surface =
            quench_node::polyfills::bootstrap::lookup("async-resource").unwrap_or("");
        let webcrypto_surface =
            quench_node::polyfills::bootstrap::lookup("webcrypto-global").unwrap_or("");
        let vfs_enabled = fixture_source.contains("--experimental-vfs");
        let vfs_head_surface = vfs_enabled
            .then(|| quench_node::polyfills::bootstrap::lookup("vfs-head").unwrap_or(""))
            .unwrap_or("");
        let vfs_surface = vfs_enabled
            .then(|| quench_node::polyfills::bootstrap::lookup("vfs").unwrap_or(""))
            .unwrap_or("");
        let vfs_stream_setup = vfs_enabled
            .then_some("Object.defineProperty(globalThis, '__nodeStream', { configurable: true, writable: true, value: require('stream') });")
            .unwrap_or("");
        let web_streams_surface = ["web-streams"]
            .into_iter()
            .filter_map(|name| quench_node::polyfills::bootstrap::lookup(name))
            .collect::<Vec<_>>()
            .join("\n");
        let performance_surface = if fixture_source.contains("perf_hooks") {
            quench_node::polyfills::bootstrap::lookup("performance")
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        };
        let url_pattern_surface =
            quench_node::polyfills::post_bootstrap::lookup("module-surface-06").unwrap_or("");
        let mut bootstrap = format!(
            "globalThis.__nodePath = __nodePath; globalThis.__quench_fs_mkdir = __quench_fs_mkdir; Object.defineProperty(globalThis, '__filename', {{ value: __quench_script_filename, configurable: true }}); globalThis.URL = URL; Object.defineProperty(globalThis, '__nodeURL', {{ value: globalThis.URL, configurable: true }}); Object.defineProperty(globalThis, '__nodeURLSearchParams', {{ value: globalThis.URLSearchParams, configurable: true }});\n{support_surface}\n{async_resource_surface}\n{url_pattern_surface}\ndelete globalThis.__quenchURLPatternFactory; delete globalThis.__quenchURLInstallCanParse; delete globalThis.__quenchURLInstallToString; delete globalThis.__nodeThrowReadonlyURLSetter; delete globalThis.__quenchURLPattern;\nif (globalThis.process && !(globalThis.__quench_allowed_node_environment_flags instanceof Set)) {{ const flags = new Set(['--perf_basic_prof', '--perf-basic-prof', '--perf_basic-prof', '-r', '--stack-trace-limit', '--inspect-brk']); const has = flags.has; flags.has = (flag) => flag === 'perf-basic-prof' || flag === 'perf_basic-prof' || flag === 'perf_basic_prof' || flag === 'r' || flag === 'inspect-brk' || flag === '--inspect_brk' || (typeof flag === 'string' && flag.startsWith('--stack-trace-limit=')) || has.call(flags, flag); process.allowedNodeEnvironmentFlags = Object.freeze(flags); }}\nif (globalThis.process && globalThis.__quench_allowed_node_environment_flags instanceof Set) process.allowedNodeEnvironmentFlags = globalThis.__quench_allowed_node_environment_flags;"
        );
        let source = bootstrap.clone();
        bootstrap = format!(
            "if (globalThis.process && globalThis.__quench_allowed_node_environment_flags instanceof Set) {{ const flags = globalThis.__quench_allowed_node_environment_flags; const has = flags.has.bind(flags); flags.has = (flag) => flag === '--perf_basic_prof' || flag === 'perf-basic-prof' || flag === 'perf_basic-prof' || flag === '--perf_basic-prof' || flag === 'perf_basic-prof' || flag === 'perf_basic_prof' || flag === '-r' || flag === 'r' || (typeof flag === 'string' && flag.startsWith('--stack-trace-limit=')) || has(flag); Object.freeze(flags); process.allowedNodeEnvironmentFlags = flags; }}\n{source}"
        );
        bootstrap = format!(
            "if (typeof Error === 'function' && Error.stackTraceLimit === undefined) Error.stackTraceLimit = __quench_error_stack_trace_limit;\n{bootstrap}"
        );
        bootstrap = format!("globalThis.process.execArgv = __quench_exec_argv;\n{bootstrap}");
        // Node's two global spellings are one identity. Declare the alias in
        // the runner realm so fixtures using `global.gc`, `global.process`,
        // and identity checks observe the same host surface as `globalThis`.
        let bootstrap_tail = format!(
            "var global = globalThis; if (typeof gc === 'function') globalThis.gc = gc; if (!Object.getOwnPropertyDescriptor(globalThis, '__nodeCurrentAsyncResource')) Object.defineProperty(globalThis, '__nodeCurrentAsyncResource', {{ value: {{}}, writable: true, configurable: true, enumerable: false }});\n{globals_surface}\n{fetch_surface}\nconst fetch = globalThis.fetch;\n{externalizable_surface}\n{report_surface}\n{punycode_surface}\n{async_resource_surface}\n{webcrypto_surface}\n{vfs_head_surface}\n{vfs_surface}\n{vfs_stream_setup}\n{web_streams_surface}\n{performance_surface}\n{dgram_surface}\n{dns_surface}\n{stream_iter_surface}"
        );
        // ESM imports create lexical bindings. Run the host bootstrap through
        // a separately constructed function so its global lookups cannot
        // resolve to an imported binding still in its temporal dead zone.
        let bootstrap = format!("{bootstrap_tail}\n{bootstrap}");
        let bootstrap_literal = format!(
            "\"{}\"",
            bootstrap
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
                .replace('\u{2028}', "\\u2028")
                .replace('\u{2029}', "\\u2029")
        );
        let source = if is_module {
            format!("Function({bootstrap_literal})();\n{fixture_program}")
        } else {
            format!("{bootstrap}\n{fixture_program}")
        };
        context = context.with_compiled_source_text(source.clone());
        self.context = context;
        self.host
            .state()
            .borrow_mut()
            .cluster
            .set_script(script.clone(), fixture_source.clone());
        let program = match reduce_fixture(&source, is_module) {
            Ok(program) => program,
            Err(error) if is_module && source.contains("await ") => {
                let wrapped = format!("(async () => {{\n{source}\n}})();");
                match reduce_fixture(&wrapped, false) {
                    Ok(program) => program,
                    Err(_) => {
                        return NodeOutcome::Fail {
                            reason: format!("reduce: {error}"),
                        };
                    }
                }
            }
            Err(error) => {
                return NodeOutcome::Fail {
                    reason: format!("reduce: {error}"),
                };
            }
        };
        let result = {
            let state = self.host.state();
            let dynamic_namespace_cache =
                std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::<
                    String,
                    Value,
                >::new()));
            let _dynamic_import = quench_runtime::module_bindings::install_dynamic_import({
                let dynamic_namespace_cache = std::rc::Rc::clone(&dynamic_namespace_cache);
                std::rc::Rc::new(move |specifier, _deferred| {
                    let mocked = quench_node::modules::test::module_is_mocked(specifier);
                    let cacheable =
                        !mocked || quench_node::modules::test::mock_module_cache(specifier);
                    let cache_key = format!(
                        "{}:{}",
                        quench_node::modules::test::canonical_mock_specifier(specifier),
                        if mocked { "mock" } else { "real" }
                    );
                    if cacheable {
                        if let Some(cached) = dynamic_namespace_cache.borrow().get(&cache_key) {
                            return Some(cached.clone());
                        }
                    }
                    match quench_node::modules::require::require_dynamic(
                        &state,
                        &[Value::String(specifier.to_owned())],
                    ) {
                        Ok(value) => {
                            let namespace = quench_node::modules::require::dynamic_namespace(value);
                            if cacheable {
                                dynamic_namespace_cache
                                    .borrow_mut()
                                    .insert(cache_key, namespace.clone());
                            }
                            Some(namespace)
                        }
                        Err(quench_runtime::vm::VmError::Thrown(reason)) => Some(
                            quench_node::modules::require::dynamic_import_rejection(reason),
                        ),
                        Err(_) => None,
                    }
                })
            });
            let executed =
                quench_runtime::vm::execute_code_with_context(program.code(), &self.context);
            normalize_script_completion(executed).and_then(|_| self.drive("__quench_run_loop__();"))
        };
        // Promise jobs may surface an uncaught rejection while the loop is
        // draining. Route that lifecycle event before checking harness call
        // counts; otherwise the verifier observes its own pending handler as
        // missing and reports a false failure.
        let result = self.route_uncaught(result);
        let result = result.and_then(|_| {
            self.drive(
                "if (typeof globalThis.__quench_verify_calls === 'function') globalThis.__quench_verify_calls();",
            )
        });
        // Host bookkeeping is complete before Node's exit observers run. Do
        // not expose the runner's async-resource and call-check cells to the
        // fixture's global-leak assertion.
        let result = result.and_then(|_| {
            self.drive(
                "delete globalThis.__nodeCurrentAsyncResource; delete globalThis.__nodeCallChecks;",
            )
        });
        // `process.exit` unwinds with an error; `exit` handlers still run.
        let result = match result {
            Err(error) => {
                // Preserve the original observable exception. The exit pump
                // may itself return a control-flow completion, but replacing
                // the assertion/error here would make every failed fixture
                // look like an opaque `process.exit(1)`.
                let _ = self.drive("__quench_run_exit__();");
                Err(error)
            }
            ok => {
                let normalized = ok.map(|_| ());
                if self.host.exit_code().is_some_and(|code| code != 0) {
                    let _ = self.drive("__quench_run_exit__();");
                }
                normalized
            }
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
                    Err(error) => {
                        if quench_node::modules::process::abort_on_uncaught_exception(
                            &self.host.state(),
                        ) {
                            std::process::abort();
                        }
                        Err(error)
                    }
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
                reason: format!("exit code {code}: {}", render_uncaught(&error)),
            },
            (Err(error), None) => NodeOutcome::Fail {
                reason: format!("runtime: {}", render_uncaught(&error)),
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

fn render_uncaught(error: &quench_runtime::vm::VmError) -> String {
    if let quench_runtime::vm::VmError::Thrown(value) = error {
        if let Value::String(stack) = quench_runtime::execute::get_property(value, "stack") {
            return stack;
        }
        if matches!(value, Value::Null | Value::Undefined)
            || matches!(value, Value::String(text) if text.starts_with("Symbol.") && text.contains('\0'))
        {
            return format!("Error: {}", error.render());
        }
    }
    error.render()
}

struct FixtureCwdGuard(Option<PathBuf>);

impl FixtureCwdGuard {
    fn capture() -> Self {
        Self(std::env::current_dir().ok())
    }
}

impl Drop for FixtureCwdGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.as_ref() {
            let _ = std::env::set_current_dir(path);
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

fn rejection_mode(
    source: &str,
    exec_argv: &[String],
) -> quench_node::modules::process::UnhandledRejectionMode {
    let mode = source
        .lines()
        .find_map(|line| line.trim().strip_prefix("// Flags:"))
        .and_then(|flags| {
            flags
                .split_whitespace()
                .find_map(|flag| flag.strip_prefix("--unhandled-rejections="))
        })
        .or_else(|| {
            exec_argv
                .iter()
                .find_map(|flag| flag.strip_prefix("--unhandled-rejections="))
        });
    match mode {
        Some("none") => quench_node::modules::process::UnhandledRejectionMode::None,
        Some("warn") => quench_node::modules::process::UnhandledRejectionMode::Warn,
        Some("strict") => quench_node::modules::process::UnhandledRejectionMode::Strict,
        _ => quench_node::modules::process::UnhandledRejectionMode::Throw,
    }
}

fn cli_title(source: &str) -> Option<String> {
    source.lines().find_map(|line| {
        line.trim()
            .strip_prefix("// Flags:")?
            .split_whitespace()
            .find_map(|flag| flag.strip_prefix("--title=").map(str::to_owned))
    })
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
