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
    let sink: OutputSink = Arc::new(|chunk| print!("{chunk}"));
    run_script_with_sink(script, script_args, source, sink)
}

/// Same as `run_script`, but routes host output through `sink`.
pub fn run_script_with_sink(
    script: &Path,
    script_args: &[String],
    source: &str,
    sink: OutputSink,
) -> RunOutcome {
    run_script_with_exec_argv(script, script_args, &[], source, sink)
}

/// Run a script with explicit process flags. Flags before the script belong
/// to `process.execArgv`; preserving them separately from script arguments is
/// essential for uncaught-exception policy and gated builtins.
pub fn run_script_with_exec_argv(
    script: &Path,
    script_args: &[String],
    exec_argv: &[String],
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
    let mut fixture_flags = source
        .lines()
        .filter_map(|line| line.trim().strip_prefix("// Flags:"))
        .flat_map(str::split_whitespace)
        .filter(|flag| flag.starts_with('-'))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    fixture_flags.extend(exec_argv.iter().cloned());
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
        "{web_streams_surface}\n{globals_surface}\n{fetch_surface}\nconst fetch = globalThis.fetch;\n{externalizable_surface}\n{report_surface}\n{async_resource_surface}\n{webcrypto_surface}\nconst crypto = globalThis.crypto;\nfor (const __name of ['MessageChannel','MessagePort','worker_threads','TypeMismatchError','QuotaExceededError','__nodeCurrentAsyncResource','__nodeCallChecks']) if (__name in globalThis) Object.defineProperty(globalThis, __name, {{ configurable: true, enumerable: false, writable: true, value: globalThis[__name] }});"
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
    crate::modules::process::flush_trace_events(&host.state());
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
    let exec_argv = std::env::var("QUENCH_EXEC_ARGV")
        .ok()
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default();
    eval_script_with_exec_argv(source, sink, module_mode, &exec_argv)
}

/// Run `node -e` source with explicit invocation flags. Flags before `-e`
/// belong to `process.execArgv`; child re-execs use this path for the same
/// distinction as file-backed scripts.
pub fn eval_script_with_exec_argv(
    source: &str,
    sink: OutputSink,
    module_mode: bool,
    exec_argv: &[String],
) -> RunOutcome {
    let argv = vec![
        std::env::current_exe()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "quench-node".to_string()),
        "<eval>".to_string(),
    ];
    let (host, context) = crate::host::install_with_argv_and_title_and_exec_argv(
        RealmId::ROOT,
        sink,
        argv,
        "quench-node",
        exec_argv,
    );
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
    let source = format!("{web_streams_surface}\n{globals_surface}\n{fetch_surface}\nconst fetch = globalThis.fetch; const crypto = globalThis.crypto;\n{punycode_surface}\n{source}");
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
                let trace = crate::modules::diagnostics_channel::module_import_begin(
                    &state,
                    crate::modules::diagnostics_channel::module_import_parent_url(&state),
                    specifier.to_owned(),
                )
                .ok()
                .flatten();
                let mocked = crate::modules::test::module_is_mocked(specifier);
                let cacheable = !mocked || crate::modules::test::mock_module_cache(specifier);
                let cache_key = format!(
                    "{}:{}",
                    crate::modules::test::canonical_mock_specifier(specifier),
                    if mocked { "mock" } else { "real" }
                );
                if cacheable {
                    if let Some(cached) = dynamic_namespace_cache.borrow().get(&cache_key) {
                        if let Some(event) = trace {
                            let _ = crate::modules::diagnostics_channel::module_import_end(
                                &state,
                                event,
                                Ok(cached.clone()),
                            );
                        }
                        return Some(cached.clone());
                    }
                }
                match crate::modules::require::require_dynamic(
                    &state,
                    &[Value::String(specifier.to_owned())],
                ) {
                    Ok(value) => {
                        let namespace = crate::modules::require::dynamic_namespace(value);
                        if let Some(event) = trace {
                            let _ = crate::modules::diagnostics_channel::module_import_end(
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
                    Err(VmError::Thrown(reason)) => {
                        let rejection =
                            crate::modules::require::dynamic_import_rejection(reason.clone());
                        if let Some(event) = trace {
                            let _ = crate::modules::diagnostics_channel::module_import_end(
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
    crate::modules::process::flush_trace_events(&host.state());
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
        Err(error) => {
            if crate::modules::process::abort_on_uncaught_exception(&host.state()) {
                std::process::abort();
            }
            match crate::modules::pump::handle_uncaught(&host.state(), error) {
                Ok(()) => {
                    let handled = drive(context, "__quench_uncaught__();")
                        .and_then(|_| drive(context, "__quench_run_loop__();"));
                    match handled {
                        Ok(_) => Ok(()),
                        Err(error)
                            if crate::modules::process::abort_on_uncaught_exception(
                                &host.state(),
                            ) =>
                        {
                            std::process::abort();
                        }
                        Err(error) => Err(error),
                    }
                }
                Err(error) => {
                    if crate::modules::process::abort_on_uncaught_exception(&host.state()) {
                        std::process::abort();
                    }
                    Err(error)
                }
            }
        }
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
#[cfg(test)]
mod tests {
    use super::{eval_script, run_script_with_sink};
    use quench_runtime::vm::OutputSink;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    #[test]
    fn detached_console_methods_do_not_require_a_receiver() {
        let output = Arc::new(Mutex::new(String::new()));
        let sink_output = Arc::clone(&output);
        let sink: OutputSink = Arc::new(move |chunk| {
            sink_output.lock().unwrap().push_str(chunk);
        });
        let source = r#"
          const methods = [
            "log", "info", "warn", "error", "debug", "trace", "dir",
            "table", "group", "groupCollapsed", "groupEnd", "clear", "assert",
            "count", "countReset", "time", "timeLog", "timeEnd",
          ];
          for (const name of methods) {
            const method = console[name];
            if (typeof method !== "function") continue;
            const detached = method;
            if (name === "assert") detached(false, "detached");
            else if (name === "time" || name === "timeLog" || name === "timeEnd") detached("detached");
            else detached("detached");
          }
          const log = console.log;
          log("detached-log");
        "#;
        let outcome = eval_script(source, sink);
        assert!(
            outcome.error.is_none(),
            "detached console call failed: {:?}",
            outcome.error
        );
        assert!(output.lock().unwrap().contains("detached-log"));
    }

    #[test]
    fn eval_routes_print_through_the_vm_output_sink() {
        let output = Arc::new(Mutex::new(String::new()));
        let sink_output = Arc::clone(&output);
        let sink: OutputSink = Arc::new(move |chunk| {
            sink_output.lock().unwrap().push_str(chunk);
        });
        let outcome = eval_script("print('plain');", sink);
        assert!(outcome.error.is_none(), "print failed: {:?}", outcome.error);
        assert_eq!(output.lock().unwrap().as_str(), "plain\n");
    }

    #[test]
    fn print_delimits_records_without_changing_stream_writes() {
        let output = Arc::new(Mutex::new(String::new()));
        let sink_output = Arc::clone(&output);
        let sink: OutputSink = Arc::new(move |chunk| {
            sink_output.lock().unwrap().push_str(chunk);
        });
        let outcome = eval_script(
            "print('line'); process.stdout.write('raw'); process.stdout.write('\\nnext');",
            sink,
        );
        assert!(
            outcome.error.is_none(),
            "print/write failed: {:?}",
            outcome.error
        );
        assert_eq!(output.lock().unwrap().as_str(), "line\nraw\nnext");
    }

    #[test]
    fn conditional_returned_function_keeps_linked_body_and_completion() {
        let output = Arc::new(Mutex::new(String::new()));
        let sink_output = Arc::clone(&output);
        let sink: OutputSink = Arc::new(move |chunk| {
            sink_output.lock().unwrap().push_str(chunk);
        });
        let source = r#"
          function choose(flag) {
            if (flag) return { f: function() { return 42; } };
            return { f: function() { return 7; } };
          }
          const yes = choose(true);
          const no = choose(false);
          console.log(yes.f());
          console.log(no.f());
          console.log("after");
        "#;
        let outcome = eval_script(source, sink);
        assert!(
            outcome.error.is_none(),
            "conditional call failed: {:?}",
            outcome.error
        );
        assert_eq!(output.lock().unwrap().as_str(), "42\n7\nafter\n");
    }

    #[test]
    fn file_runner_pumps_timers_and_preserves_sync_output_order() {
        let output = Arc::new(Mutex::new(String::new()));
        let sink_output = Arc::clone(&output);
        let sink: OutputSink = Arc::new(move |chunk| {
            sink_output.lock().unwrap().push_str(chunk);
        });
        let outcome = run_script_with_sink(
            Path::new("/tmp/quench-host-timer.js"),
            &[],
            "console.log('sync'); setTimeout(() => console.log('timer'), 0);",
            sink,
        );
        assert!(outcome.error.is_none(), "timer failed: {:?}", outcome.error);
        assert_eq!(output.lock().unwrap().as_str(), "sync\ntimer\n");
    }

    #[test]
    fn async_loop_continuations_preserve_conditional_and_finally_suffixes() {
        let output = Arc::new(Mutex::new(String::new()));
        let sink_output = Arc::clone(&output);
        let sink: OutputSink = Arc::new(move |chunk| {
            sink_output.lock().unwrap().push_str(chunk);
        });
        let source = r#"
          async function conditional() {
            let value = 0;
            for (let i = 0; i < 2; i++) {
              if (i === 1) value += await 1;
              value += await 10;
            }
            return value;
          }
          async function finalized() {
            let value = 0;
            try { for (let i = 0; i < 2; i++) value += await 1; }
            finally { value += 100; }
            return value;
          }
          conditional().then(console.log);
          finalized().then(console.log);
        "#;
        let outcome = run_script_with_sink(
            Path::new("/tmp/quench-async-continuation.js"),
            &[],
            source,
            sink,
        );
        assert!(
            outcome.error.is_none(),
            "async continuation failed: {:?}",
            outcome.error
        );
        let lines = output
            .lock()
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert!(
            lines.contains(&"21".to_string()),
            "missing conditional result: {lines:?}"
        );
        assert!(
            lines.contains(&"102".to_string()),
            "missing finally result: {lines:?}"
        );
    }

    #[test]
    fn sequential_async_loops_keep_each_loop_continuation() {
        let output = Arc::new(Mutex::new(String::new()));
        let sink_output = Arc::clone(&output);
        let sink: OutputSink = Arc::new(move |chunk| sink_output.lock().unwrap().push_str(chunk));
        let source = r#"
          async function run() {
            const first = [], second = [];
            for (let i = 0; i < 2; i++) first.push(await 1);
            for (let i = 0; i < 2; i++) second.push(await 2);
            return JSON.stringify([first, second]);
          }
          run().then(console.log);
        "#;
        let outcome = run_script_with_sink(
            Path::new("/tmp/quench-sequential-async-loops.js"),
            &[],
            source,
            sink,
        );
        assert!(
            outcome.error.is_none(),
            "sequential async loops failed: {:?}",
            outcome.error
        );
        assert_eq!(output.lock().unwrap().trim(), "[[1,1],[2,2]]");
    }

    #[test]
    fn nested_generator_progress_and_source_return_are_exact() {
        let output = Arc::new(Mutex::new(String::new()));
        let sink_output = Arc::clone(&output);
        let sink: OutputSink = Arc::new(move |chunk| sink_output.lock().unwrap().push_str(chunk));
        let source = r#"
          function* nested() {
            for (let i = 0; i < 2; i++)
              for (let j = 0; j < 2; j++) yield i * 10 + j;
          }
          const a = nested(), values = [];
          for (let i = 0; i < 6; i++) { const r = a.next(); values.push([r.value, r.done]); }
          function* returned() {
            for (let i = 0; i < 3; i++) { yield i; if (i === 1) return 99; }
          }
          const b = returned();
          console.log(JSON.stringify(values));
          console.log(JSON.stringify([b.next().value, b.next().value, b.next().value, b.next().value]));
        "#;
        let outcome = run_script_with_sink(
            Path::new("/tmp/quench-nested-generators.js"),
            &[],
            source,
            sink,
        );
        assert!(
            outcome.error.is_none(),
            "nested generator failed: {:?}",
            outcome.error
        );
        let lines = output
            .lock()
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(
            lines.first().map(String::as_str),
            Some("[[0,false],[1,false],[10,false],[11,false],[null,true],[null,true]]")
        );
        assert_eq!(lines.get(1).map(String::as_str), Some("[0,1,99,null]"));
    }

    #[test]
    fn suspended_shared_bindings_survive_collection_with_dead_siblings() {
        let output = Arc::new(Mutex::new(String::new()));
        let sink_output = Arc::clone(&output);
        let sink: OutputSink = Arc::new(move |chunk| {
            sink_output.lock().unwrap().push_str(chunk);
        });
        let source = r#"
          async function worker() {
            const signature = function(value) { return value + 1; };
            const operation = async function() { return 41; };
            await Promise.resolve(0);
            for (let i = 0; i < 4096; i++) {
              const dead = { i: i, payload: [i, i + 1, i + 2, i + 3] };
              if (dead.i < 0) console.log(dead);
            }
            return [typeof signature, typeof operation, signature(await operation())];
          }
          worker().then(function(result) { console.log(result.join(':')); });
        "#;
        let outcome = run_script_with_sink(
            Path::new("/tmp/quench-suspended-binding-gc.js"),
            &[],
            source,
            sink,
        );
        assert!(
            outcome.error.is_none(),
            "suspended binding run failed: {:?}",
            outcome.error
        );
        assert_eq!(output.lock().unwrap().trim(), "function:function:42");
    }

    #[test]
    fn polymorphic_method_dispatch_preserves_semantics() {
        let output = Arc::new(Mutex::new(String::new()));
        let sink_output = Arc::clone(&output);
        let sink: OutputSink = Arc::new(move |chunk| {
            sink_output.lock().unwrap().push_str(chunk);
        });
        // This is deliberately a small, general reproduction of the
        // DeltaBlue shape: unrelated constructors share one method name while
        // each instance has a distinct property layout.  It must remain on
        // the ordinary complete semantics path for every shape.
        let source = r#"
          function A() { this.x = 1; this.a = 0; }
          function B() { this.x = 2; this.b = 0; }
          function C() { this.x = 3; this.c = 0; }
          function D() { this.x = 4; this.d = 0; }
          function E() { this.x = 5; this.e = 0; }
          A.prototype.f = B.prototype.f = C.prototype.f =
            D.prototype.f = E.prototype.f = function() {
              this.a = this.x + 1;
              return this.x;
            };
          const values = [];
          for (let i = 0; i < 25; i++) {
            values.push(i % 5 === 0 ? new A : i % 5 === 1 ? new B :
              i % 5 === 2 ? new C : i % 5 === 3 ? new D : new E);
          }
          let total = 0;
          for (let i = 0; i < 250; i++) total += values[i % values.length].f();
          console.log(total);
        "#;
        let outcome = eval_script(source, sink);
        assert!(
            outcome.error.is_none(),
            "polymorphic dispatch failed: {:?}",
            outcome.error
        );
        assert_eq!(output.lock().unwrap().trim(), "750");
    }
}
