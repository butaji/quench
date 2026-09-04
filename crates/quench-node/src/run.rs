//! Canonical `node <script>` runner shared by the `quench-node` CLI
//! binary and any host front-end. This is the single place that owns
//! the end-to-end pipeline: install the host with `node <script>`
//! argv semantics, run the script as a CJS module, pump the event
//! loop, run `exit` handlers, and resolve the process exit code.

use std::path::Path;
use std::sync::Arc;

use quench_runtime::ops::RealmId;
use quench_runtime::value::Value;
use quench_runtime::vm::{execute_code_with_context, OutputSink, VmContext, VmError};

/// One `node <script>` run: the resolved process exit code plus an
/// optional rendered error for stderr.
pub struct RunOutcome {
    pub exit_code: i32,
    pub error: Option<String>,
}

impl RunOutcome {
    fn success() -> Self {
        Self {
            exit_code: 0,
            error: None,
        }
    }

    fn ok(exit_code: i32) -> Self {
        Self {
            exit_code,
            error: None,
        }
    }

    fn fail(exit_code: i32, error: String) -> Self {
        Self {
            exit_code,
            error: Some(error),
        }
    }
}

/// Run `source` as a CJS module at `script`, printing console output
/// to stdout.
pub fn run_script(script: &Path, script_args: &[String], source: &str) -> RunOutcome {
    let sink: OutputSink = Arc::new(|line| println!("{line}"));
    run_script_with_sink(script, script_args, source, sink)
}

/// Same as `run_script`, but routes host output through `sink`.
pub fn run_script_with_sink(
    script: &Path,
    script_args: &[String],
    source: &str,
    sink: OutputSink,
) -> RunOutcome {
    // Compatibility tests model the Node executable, not the test harness
    // binary that happens to host it.
    let exec = "quench-node".to_string();
    let script_str = script.to_string_lossy().into_owned();
    let mut argv = vec![exec, script_str.clone()];
    argv.extend(script_args.iter().cloned());
    let title = source
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("// Flags:")?
                .split_whitespace()
                .find_map(|flag| flag.strip_prefix("--title=").map(str::to_owned))
        })
        .unwrap_or_else(|| "quench-node".into());
    let fixture_flags = source
        .lines()
        .filter_map(|line| line.trim().strip_prefix("// Flags:"))
        .flat_map(str::split_whitespace)
        .filter(|flag| flag.starts_with('-'))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let (host, context) = crate::host::install_with_argv_and_title_and_exec_argv(
        RealmId::ROOT,
        sink,
        argv,
        &title,
        &fixture_flags,
    );
    // The upstream Node runner treats `// Flags:` as invocation metadata,
    // not as script arguments.  Keep that distinction in the canonical
    // runner: flags belong to `process.execArgv`, while `process.argv`
    // remains `[execPath, script, ...args]`.  This also lets gated built-ins
    // (for example `stream/iter`) observe the same fact as their Node oracle.
    let context = context
        .with_source_text(source.to_owned())
        .with_source_name(script_str.clone());
    if let Some(dir) = script.parent() {
        host.set_main_dir(dir.to_string_lossy().into_owned());
    }
    let wrapped = crate::modules::require::wrap_cjs(&host.state(), &script_str, source);
    let url_pattern_surface =
        crate::polyfills::post_bootstrap::lookup("module-surface-06").unwrap_or("");
    let globals_surface = crate::polyfills::bootstrap::lookup("globals-extra").unwrap_or("");
    let fetch_surface = crate::polyfills::bootstrap::lookup("fetch").unwrap_or("");
    let externalizable_surface = source
        .contains("Externalizable")
        .then(|| crate::polyfills::bootstrap::lookup("externalizable-strings").unwrap_or(""))
        .unwrap_or("");
    let web_streams_surface = crate::polyfills::bootstrap::lookup("web-streams").unwrap_or("");
    let report_surface = crate::polyfills::bootstrap::lookup("report").unwrap_or("");
    let punycode_surface = crate::polyfills::bootstrap::lookup("punycode").unwrap_or("");
    let async_resource_surface =
        crate::polyfills::bootstrap::lookup("async-resource").unwrap_or("");
    let webcrypto_surface = crate::polyfills::bootstrap::lookup("webcrypto-global").unwrap_or("");
    let vfs_enabled = source.contains("--experimental-vfs");
    let vfs_head_surface = vfs_enabled
        .then(|| crate::polyfills::bootstrap::lookup("vfs-head").unwrap_or(""))
        .unwrap_or("");
    let vfs_surface = vfs_enabled
        .then(|| crate::polyfills::bootstrap::lookup("vfs").unwrap_or(""))
        .unwrap_or("");
    let vfs_stream_setup = vfs_enabled
        .then_some("Object.defineProperty(globalThis, '__nodeStream', { configurable: true, writable: true, value: require('stream') });")
        .unwrap_or("");
    let performance_surface = crate::polyfills::bootstrap::lookup("performance").unwrap_or("");
    let persistent_globals = crate::registry::PERSISTENT_GLOBALS
        .iter()
        .map(|spec| {
            let name = spec.name.rsplit([':', '.']).next().unwrap_or(spec.name);
            format!("globalThis[{name:?}] = globalThis[{name:?}];")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let bootstrap_surface = format!(
        "{web_streams_surface}\n{globals_surface}\n{fetch_surface}\nconst fetch = globalThis.fetch;\n{externalizable_surface}\n{report_surface}\n{async_resource_surface}\n{webcrypto_surface}\nfor (const __name of ['MessageChannel','MessagePort','worker_threads','TypeMismatchError','QuotaExceededError','__nodeCurrentAsyncResource','__nodeCallChecks']) if (__name in globalThis) Object.defineProperty(globalThis, __name, {{ configurable: true, enumerable: false, writable: true, value: globalThis[__name] }});"
    );
    let wrapped = format!(
        "{bootstrap_surface}\n{punycode_surface}\n{vfs_head_surface}\n{vfs_surface}\n{vfs_stream_setup}\nObject.defineProperty(globalThis, '__nodePath', {{ value: __nodePath, configurable: true, enumerable: false }}); Object.defineProperty(globalThis, '__quench_fs_mkdir', {{ value: __quench_fs_mkdir, configurable: true, enumerable: false }}); globalThis.URL = URL; Object.defineProperty(globalThis, '__nodeURL', {{ value: globalThis.URL, configurable: true }}); Object.defineProperty(globalThis, '__nodeURLSearchParams', {{ value: globalThis.URLSearchParams, configurable: true }});\n{performance_surface}\n{url_pattern_surface}\nObject.defineProperty(globalThis, '__quenchURLPattern', {{ value: globalThis.__quenchURLPatternFactory?.(), configurable: true }}); delete globalThis.__quenchURLPatternFactory; delete globalThis.__quenchURLInstallCanParse; delete globalThis.__quenchURLInstallToString; delete globalThis.__nodeThrowReadonlyURLSetter;\n{wrapped}\n// Materialize persistent host globals after module setup and before the pump.\n{persistent_globals}"
    );
    let context = context.with_compiled_source_text(wrapped.clone());
    let ops = match reduce(&wrapped) {
        Ok(ops) => ops,
        Err(error) => return RunOutcome::fail(1, format!("reduce: {error}")),
    };
    let result = quench_runtime::vm::with_current_context(&context, || {
        normalize_script_completion(execute_code_with_context(ops.code(), &context))
            .and_then(|_| drive(&context, "__quench_run_loop__();"))
    });
    let result = route_uncaught(&host, &context, result);
    let result = match result {
        Err(error) => match drive(&context, "__quench_run_exit__();") {
            Ok(_) => Err(error),
            Err(exit_error) => Err(exit_error),
        },
        ok => ok.map(|_| ()),
    };
    sync_process_exit_code(&host);
    classify(result, host.exit_code())
}

/// Run eval source directly in the canonical installed Node context.
pub fn eval_script(source: &str, sink: OutputSink) -> RunOutcome {
    eval_script_with_input_type(source, sink, false)
}

pub fn eval_script_with_input_type(
    source: &str,
    sink: OutputSink,
    module_mode: bool,
) -> RunOutcome {
    let argv = vec![
        std::env::current_exe()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "quench-node".to_string()),
        "<eval>".to_string(),
    ];
    let (host, context) = crate::host::install_with_argv(RealmId::ROOT, sink, argv);
    let context = context
        .with_source_text(source.to_owned())
        .with_source_name("<eval>");
    // `node -e` resolves bare modules from the process cwd, just like a
    // script resolves them from its containing directory.
    if let Ok(cwd) = std::env::current_dir() {
        host.set_main_dir(cwd.to_string_lossy().into_owned());
    }
    let globals_surface = crate::polyfills::bootstrap::lookup("globals-extra").unwrap_or("");
    let fetch_surface = crate::polyfills::bootstrap::lookup("fetch").unwrap_or("");
    let web_streams_surface = crate::polyfills::bootstrap::lookup("web-streams").unwrap_or("");
    let source_text = source.to_owned();
    let punycode_surface = crate::polyfills::bootstrap::lookup("punycode").unwrap_or("");
    let source = format!("{web_streams_surface}\n{globals_surface}\n{fetch_surface}\nconst fetch = globalThis.fetch;\n{punycode_surface}\n{source}");
    let context = context.with_compiled_source_text(source.clone());
    let ops = match reduce(&source) {
        Ok(ops) => ops,
        Err(error) => return RunOutcome::fail(1, format!("reduce: {error}")),
    };
    let result = crate::modules::require::with_static_esm_mode(module_mode, || {
        let state = host.state();
        let dynamic_namespace_cache =
            std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::<
                String,
                Value,
            >::new()));
        let _dynamic_import = quench_runtime::module_bindings::install_dynamic_import({
            let dynamic_namespace_cache = std::rc::Rc::clone(&dynamic_namespace_cache);
            std::rc::Rc::new(move |specifier, _deferred| {
                let mocked = crate::modules::test::module_is_mocked(specifier);
                let cacheable = !mocked || crate::modules::test::mock_module_cache(specifier);
                let cache_key = format!(
                    "{}:{}",
                    crate::modules::test::canonical_mock_specifier(specifier),
                    if mocked { "mock" } else { "real" }
                );
                if cacheable {
                    if let Some(cached) = dynamic_namespace_cache.borrow().get(&cache_key) {
                        return Some(cached.clone());
                    }
                }
                match crate::modules::require::require_dynamic(
                    &state,
                    &[Value::String(specifier.to_owned())],
                ) {
                    Ok(value) => {
                        let namespace = crate::modules::require::dynamic_namespace(value);
                        if cacheable {
                            dynamic_namespace_cache
                                .borrow_mut()
                                .insert(cache_key, namespace.clone());
                        }
                        Some(namespace)
                    }
                    Err(VmError::Thrown(reason)) => {
                        Some(crate::modules::require::dynamic_import_rejection(reason))
                    }
                    Err(_) => None,
                }
            })
        });
        normalize_script_completion(execute_code_with_context(ops.code(), &context))
            .and_then(|_| drive(&context, "__quench_run_loop__();"))
    });
    let result = route_uncaught(&host, &context, result);
    let result = match result {
        Err(error) => {
            let _ = drive(&context, "__quench_run_exit__();");
            Err(decorate_eval_error(error, &source_text))
        }
        Ok(_) => Ok(()),
    };
    sync_process_exit_code(&host);
    classify(result, host.exit_code())
}

/// Eval-mode child processes have no script filename to feed the VM's normal
/// source-location path.  Preserve the observable multiline syntax stack so
/// internal error decorators and callers still see the offending source line.
fn decorate_eval_error(error: VmError, source: &str) -> VmError {
    let VmError::Thrown(value) = &error else {
        return error;
    };
    if !matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
        return error;
    }
    let name = quench_runtime::execute::to_js_string(&quench_runtime::execute::get_property(
        value, "name",
    ))
    .unwrap_or_else(|_| "Error".into());
    let message = quench_runtime::execute::to_js_string(&quench_runtime::execute::get_property(
        value, "message",
    ))
    .unwrap_or_default();
    let first_line = source.lines().next().unwrap_or_default();
    let display_line = if first_line.contains(';') {
        first_line.to_string()
    } else {
        format!("{first_line};")
    };
    let stack = format!("<eval>:1\n{display_line}\n ^\n\n{name}: {message}\n    at <eval>:1:1");
    let updated =
        quench_runtime::execute::set_property(value.clone(), "stack", Value::String(stack));
    quench_runtime::execute::replace_value(value, &updated);
    error
}

fn sync_process_exit_code(host: &crate::host::NodeHost) {
    let global = quench_runtime::vm::current_global_object();
    let process = quench_runtime::execute::get_property(&global, "process");
    if let Value::Number(code) = quench_runtime::execute::get_property(&process, "exitCode") {
        host.state().borrow_mut().process.exit_code = Some(code as i32);
    }
}

/// Node dispatches top-level uncaught exceptions to
/// `process.on('uncaughtException')`; a handled run continues.
fn route_uncaught(
    host: &crate::host::NodeHost,
    context: &VmContext,
    result: Result<quench_runtime::value::Value, VmError>,
) -> Result<(), VmError> {
    match result {
        Err(error) => match crate::modules::pump::handle_uncaught(&host.state(), error) {
            Ok(()) => {
                drive(context, "__quench_uncaught__();")
                    .and_then(|_| drive(context, "__quench_run_loop__();"))?;
                Ok(())
            }
            Err(error) => {
                if crate::modules::process::abort_on_uncaught_exception(&host.state()) {
                    std::process::abort();
                }
                Err(error)
            }
        },
        ok => ok.map(|_| ()),
    }
}

fn classify(result: Result<(), VmError>, exit_code: Option<i32>) -> RunOutcome {
    match (result, exit_code) {
        // `process.exit(code)` sets the code regardless of the unwind; a
        // set code is honored silently, matching Node's CLI.
        (_, Some(code)) => RunOutcome::ok(code),
        (Ok(_), None) => RunOutcome::success(),
        // Top-level uncaught exception: report it and exit 1.
        (Err(error), None) => RunOutcome::fail(1, uncaught_render(&error)),
    }
}

fn uncaught_render(error: &VmError) -> String {
    if let VmError::Thrown(value) = error {
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

/// Execute a tiny driver snippet (e.g. `__quench_run_loop__();`) in
/// the run context so the pump runs inside an active execution frame.
fn drive(context: &VmContext, source: &str) -> Result<quench_runtime::value::Value, VmError> {
    let ops = reduce(source).map_err(VmError::EvalError)?;
    match execute_code_with_context(ops.code(), context) {
        // Driver snippets are statements.  A normal statement completion has
        // no value, but the VM exposes that completion as MissingReturn.
        Err(VmError::MissingReturn) => Ok(quench_runtime::value::Value::Undefined),
        result => result,
    }
}

fn normalize_script_completion(
    result: Result<quench_runtime::value::Value, VmError>,
) -> Result<quench_runtime::value::Value, VmError> {
    match result {
        Err(VmError::MissingReturn) => Ok(quench_runtime::value::Value::Undefined),
        result => result,
    }
}

fn reduce(source: &str) -> Result<quench_runtime::reduce::ResidualProgram, String> {
    quench_runtime::reduce::reduce_source(source).map_err(|errors| errors.join("; "))
}
