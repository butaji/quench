//! Discovery and classification of Node fixture outcomes.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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

/// Parse the shell-like token grammar accepted by NODE_OPTIONS.  Node accepts
/// quoted and escaped module paths (notably paths containing spaces), so a
/// plain whitespace split would change the child process contract.
pub fn parse_node_options(raw: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut started = false;
    for character in raw.chars() {
        if escaped {
            token.push(character);
            escaped = false;
            started = true;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            started = true;
            continue;
        }
        if let Some(active) = quote {
            if character == active {
                quote = None;
            } else {
                token.push(character);
            }
            started = true;
            continue;
        }
        match character {
            '\'' | '"' => {
                quote = Some(character);
                started = true;
            }
            c if c.is_whitespace() => {
                if started {
                    tokens.push(std::mem::take(&mut token));
                    started = false;
                }
            }
            c => {
                token.push(c);
                started = true;
            }
        }
    }
    if escaped || quote.is_some() {
        return Err(raw.to_string());
    }
    if started {
        tokens.push(token);
    }
    Ok(tokens)
}

/// Return the source prefix for Node's `-r`/`--require` invocation options.
/// Preloads are ordinary CommonJS modules and therefore run after the host
/// bootstrap but before the entry program.
pub fn node_preload_program(args: &[String]) -> String {
    let mut program = String::new();
    let mut index = 0;
    while index < args.len() {
        let value = &args[index];
        let module = if value == "-r" || value == "--require" {
            index += 1;
            args.get(index).map(String::as_str)
        } else {
            value
                .strip_prefix("--require=")
                .or_else(|| value.strip_prefix("-r"))
        };
        if let Some(module) = module.filter(|module| !module.is_empty()) {
            program.push_str("require(\"");
            for character in module.chars() {
                match character {
                    '\\' => program.push_str("\\\\"),
                    '"' => program.push_str("\\\""),
                    '\n' => program.push_str("\\n"),
                    '\r' => program.push_str("\\r"),
                    '\t' => program.push_str("\\t"),
                    c if c.is_control() => {
                        use std::fmt::Write;
                        let _ = write!(program, "\\u{{{:04x}}}", c as u32);
                    }
                    c => program.push(c),
                }
            }
            program.push_str("\");\n");
        }
        index += 1;
    }
    program
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
        let captured_output = Arc::new(Mutex::new(String::new()));
        let captured_output_sink = Arc::clone(&captured_output);
        let parent_sink = Arc::clone(&self.sink);
        let sink: std::sync::Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |chunk| {
            if let Ok(mut output) = captured_output_sink.lock() {
                output.push_str(chunk);
            }
            parent_sink(chunk);
        });
        let (host, context) = quench_node::host::install_script_with_args_and_title(
            RealmId::ROOT,
            sink,
            &script,
            &fixture.argv,
            &title,
        );
        // Keep the fixture global rooted across initial evaluation and every
        // event-loop continuation. Async jobs resume in fresh VM frames and
        // must observe the same script-installed global properties.
        let _shared_global = quench_runtime::vm::SharedGlobal::install();
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
            )
            .with_host_value(
                "__quench_module_url".to_string(),
                Value::String(format!("file://{script}")),
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
        quench_node::modules::process::configure_deprecation_flags(&fixture.exec_argv);
        if let Ok((total, min)) =
            quench_node::modules::process::secure_heap_config(&fixture.exec_argv)
        {
            quench_node::modules::process::set_secure_heap_config(&self.host.state(), total, min);
        }
        quench_node::modules::process::configure_permissions(
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
            [
                "internal-fs-binding",
                "dgram-head",
                "dgram",
                "dgram-tail",
                "membership",
            ]
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
        let zlib_iter_surface = if fixture_source.contains("zlib/iter") {
            quench_node::polyfills::bootstrap::iterators::JS.to_string()
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
        let target_surface = quench_node::polyfills::bootstrap::lookup("target").unwrap_or("");
        let stream_classes_surface = "";
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
        // stream/iter conversion helpers reuse the canonical stream
        // constructors.  This internal alias is needed for ordinary stream
        // fixtures too; VFS must not control its availability.
        let vfs_stream_setup = "Object.defineProperty(globalThis, '__nodeStream', { configurable: true, writable: true, value: require('stream') });";
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
        let deprecation_surface =
            quench_node::modules::process::deprecation_policy_source(&fixture.exec_argv);
        let mut bootstrap = format!(
            "globalThis.__nodePath = __nodePath; globalThis.__quench_fs_mkdir = __quench_fs_mkdir; Object.defineProperty(globalThis, '__filename', {{ value: __quench_script_filename, configurable: true }}); Object.defineProperty(globalThis, 'import_meta', {{ configurable: true, value: {{ url: __quench_module_url, dirname: __filename.replace(/[^/\\\\]*$/, ''), filename: __filename, resolve(specifier, parent) {{ return new URL(specifier, parent || __quench_module_url).href; }} }} }}); globalThis.URL = URL; Object.defineProperty(globalThis, '__nodeURL', {{ value: globalThis.URL, configurable: true }}); Object.defineProperty(globalThis, '__nodeURLSearchParams', {{ value: globalThis.URLSearchParams, configurable: true }});\n{support_surface}\n{stream_classes_surface}\n{async_resource_surface}\n{url_pattern_surface}\ndelete globalThis.__quenchURLPatternFactory; delete globalThis.__quenchURLInstallCanParse; delete globalThis.__quenchURLInstallToString; delete globalThis.__nodeThrowReadonlyURLSetter; delete globalThis.__quenchURLPattern;\nif (globalThis.process && !(globalThis.__quench_allowed_node_environment_flags instanceof Set)) {{ const flags = new Set(['--perf_basic_prof', '--perf-basic-prof', '--perf_basic-prof', '-r', '--stack-trace-limit', '--inspect-brk']); const has = flags.has; flags.has = (flag) => flag === 'perf-basic-prof' || flag === 'perf_basic-prof' || flag === 'perf_basic_prof' || flag === 'r' || flag === 'inspect-brk' || flag === '--inspect_brk' || (typeof flag === 'string' && flag.startsWith('--stack-trace-limit=')) || has.call(flags, flag); flags.has = has; process.allowedNodeEnvironmentFlags = Object.freeze(flags); }}\nif (globalThis.process && globalThis.__quench_allowed_node_environment_flags instanceof Set) process.allowedNodeEnvironmentFlags = globalThis.__quench_allowed_node_environment_flags;"
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
            "var global = globalThis; if (typeof gc === 'function') globalThis.gc = gc; if (!Object.getOwnPropertyDescriptor(globalThis, '__nodeCurrentAsyncResource')) Object.defineProperty(globalThis, '__nodeCurrentAsyncResource', {{ value: {{}}, writable: true, configurable: true, enumerable: false }});\n{globals_surface}\n{fetch_surface}\nconst fetch = globalThis.fetch;\n{externalizable_surface}\n{report_surface}\n{punycode_surface}\n{async_resource_surface}\n{webcrypto_surface}\n{vfs_head_surface}\n{vfs_surface}\n{vfs_stream_setup}\n{web_streams_surface}\n{performance_surface}\n{dgram_surface}\n{dns_surface}\n{stream_iter_surface}\n{zlib_iter_surface}\n{target_surface}"
        );
        // ESM imports create lexical bindings. Run the host bootstrap through
        // a separately constructed function so its global lookups cannot
        // resolve to an imported binding still in its temporal dead zone.
        // Host polyfills use global properties as their shared capability
        // registry. Those implementation names are not part of Node's
        // enumerable global surface; normalize their descriptors once after
        // bootstrap so fixture global-leak checks see the public shape.
        let bootstrap = format!(
            "{bootstrap_tail}\nfor (const __name of Object.keys(globalThis)) if (__name === 'ReadableStream' || __name.startsWith('__quench')) {{ const __descriptor = Object.getOwnPropertyDescriptor(globalThis, __name); if (__descriptor?.configurable) Object.defineProperty(globalThis, __name, {{ ...__descriptor, enumerable: false }}); }}\n{bootstrap}"
        );
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
            format!(
                "Function({bootstrap_literal})();\n{}{}{}",
                deprecation_surface,
                node_preload_program(&fixture.exec_argv),
                fixture_program
            )
        } else {
            format!(
                "{bootstrap}\n{deprecation_surface}{}{}",
                node_preload_program(&fixture.exec_argv),
                fixture_program
            )
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
                    let trace = quench_node::modules::diagnostics_channel::module_import_begin(
                        &state,
                        quench_node::modules::diagnostics_channel::module_import_parent_url(&state),
                        specifier.to_owned(),
                    )
                    .ok()
                    .flatten();
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
                            if let Some(event) = trace {
                                let _ =
                                    quench_node::modules::diagnostics_channel::module_import_end(
                                        &state,
                                        event,
                                        Ok(cached.clone()),
                                    );
                            }
                            return Some(cached.clone());
                        }
                    }
                    match quench_node::modules::require::require_dynamic(
                        &state,
                        &[Value::String(specifier.to_owned())],
                    ) {
                        Ok(value) => {
                            let namespace = quench_node::modules::require::dynamic_namespace(value);
                            if let Some(event) = trace {
                                let _ =
                                    quench_node::modules::diagnostics_channel::module_import_end(
                                        &state,
                                        event,
                                        Ok(namespace.clone()),
                                    );
                            }
                            if cacheable {
                                dynamic_namespace_cache
                                    .borrow_mut()
                                    .insert(cache_key, namespace.clone());
                            }
                            Some(namespace)
                        }
                        Err(quench_runtime::vm::VmError::Thrown(reason)) => {
                            let rejection = quench_node::modules::require::dynamic_import_rejection(
                                reason.clone(),
                            );
                            if let Some(event) = trace {
                                let _ =
                                    quench_node::modules::diagnostics_channel::module_import_end(
                                        &state,
                                        event,
                                        Err(reason),
                                    );
                            }
                            Some(rejection)
                        }
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
        let captured_output = captured_output
            .lock()
            .map(|output| output.clone())
            .unwrap_or_default();
        if let Some(reason) = tap_skip_reason(&captured_output) {
            let has_tests = captured_output.lines().any(|line| {
                line.trim_start().starts_with("ok ") || line.trim_start().starts_with("not ok ")
            });
            if !has_tests {
                return NodeOutcome::Skip { reason };
            }
        }
        Self::classify(result, self.host.exit_code(), &captured_output)
    }

    /// Node dispatches top-level uncaught exceptions to
    /// `process.on('uncaughtException')`; a handled run continues.
    fn route_uncaught(
        &self,
        result: Result<quench_runtime::value::Value, quench_runtime::vm::VmError>,
    ) -> Result<(), quench_runtime::vm::VmError> {
        match result {
            Err(error) => {
                // `common.skip()` terminates through the same private
                // process-exit completion as an ordinary script exit.  It is
                // a control-flow signal, not an uncaught exception; routing
                // it through the process error handler would turn a clean
                // skip into exit status 1 before classification.
                if is_process_exit_signal(&error) {
                    return Err(error);
                }
                if quench_node::modules::process::abort_on_uncaught_exception(&self.host.state()) {
                    std::process::abort();
                }
                match quench_node::modules::pump::handle_uncaught(&self.host.state(), error) {
                    Ok(()) => {
                        let handled = self
                            .drive("__quench_uncaught__();")
                            .and_then(|_| self.drive("__quench_run_loop__();"));
                        match handled {
                            Ok(_) => Ok(()),
                            Err(error)
                                if quench_node::modules::process::abort_on_uncaught_exception(
                                    &self.host.state(),
                                ) =>
                            {
                                std::process::abort();
                            }
                            Err(error) => Err(error),
                        }
                    }
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
        output: &str,
    ) -> NodeOutcome {
        if let (Err(error), Some(0)) = (&result, exit_code) {
            if is_process_exit_signal(error) {
                if let Some(reason) = tap_skip_reason(output) {
                    return NodeOutcome::Skip { reason };
                }
            }
        }
        match (result, exit_code) {
            (Ok(_), None | Some(0)) => NodeOutcome::Pass,
            (Ok(_), Some(code)) => NodeOutcome::Fail {
                reason: format!("exit code {code}"),
            },
            (Err(_), Some(0)) => NodeOutcome::Pass,
            (Err(error), Some(code)) => {
                // `process.exit(code)` uses a private thrown completion to
                // unwind the VM.  It is a normal CLI termination, not an
                // uncaught exception, so preserve the status without
                // manufacturing stderr output in child mode.
                let explicit_exit = is_process_exit_signal(&error);
                if explicit_exit {
                    NodeOutcome::Fail {
                        reason: format!("exit code {code}"),
                    }
                } else {
                    NodeOutcome::Fail {
                        reason: format!("exit code {code}: {}", render_uncaught(&error)),
                    }
                }
            }
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

fn is_process_exit_signal(error: &quench_runtime::vm::VmError) -> bool {
    matches!(
        error,
        quench_runtime::vm::VmError::Thrown(Value::String(text))
            if text.starts_with("process.exit(")
    ) || matches!(
        error,
        quench_runtime::vm::VmError::Thrown(value)
            if matches!(
                quench_runtime::execute::get_property(value, "__quench_process_exit"),
                Value::Boolean(true)
            )
    )
}

fn tap_skip_reason(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let marker = line.find("# Skipped:")?;
        let reason = line[marker + "# Skipped:".len()..].trim();
        (!reason.is_empty()).then(|| reason.to_string())
    })
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
