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
    // OutputSink carries raw stream chunks.  `process.stdout.write` may omit
    // a newline, so adding one here would change observable Node behavior.
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
    // Compatibility tests model the Node executable, not the test harness
    // binary that happens to host it.
    let exec = "quench-node".to_string();
    let script_str = script.to_string_lossy().into_owned();
    let mut argv = vec![exec, script_str.clone()];
    argv.extend(script_args.iter().cloned());
    let (host, context) = crate::host::install_with_argv(RealmId::ROOT, sink, argv);
    let context = context.with_source_text(source.to_owned());
    if let Some(dir) = script.parent() {
        host.set_main_dir(dir.to_string_lossy().into_owned());
    }

    let wrapped = crate::modules::require::wrap_cjs(&host.state(), &script_str, source);
    let url_pattern_surface =
        crate::polyfills::post_bootstrap::lookup("module-surface-06").unwrap_or("");
    let persistent_globals = crate::registry::PERSISTENT_GLOBALS
        .iter()
        .map(|spec| {
            let name = spec.name.rsplit([':', '.']).next().unwrap_or(spec.name);
            format!("globalThis[{name:?}] = globalThis[{name:?}];")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let wrapped = format!(
        "globalThis.URL = URL; Object.defineProperty(globalThis, '__nodeURL', {{ value: globalThis.URL, configurable: true }}); Object.defineProperty(globalThis, '__nodeURLSearchParams', {{ value: globalThis.URLSearchParams, configurable: true }});\n{url_pattern_surface}\nObject.defineProperty(globalThis, '__quenchURLPattern', {{ value: globalThis.__quenchURLPatternFactory?.(), configurable: true }}); delete globalThis.__quenchURLPatternFactory; delete globalThis.__quenchURLInstallCanParse; delete globalThis.__quenchURLInstallToString; delete globalThis.__nodeThrowReadonlyURLSetter;\n{wrapped}\n// Materialize persistent host globals after module setup and before the pump.\n{persistent_globals}"
    );
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
    let argv = vec![
        std::env::current_exe()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "quench-node".to_string()),
        "<eval>".to_string(),
    ];
    let (host, context) = crate::host::install_with_argv(RealmId::ROOT, sink, argv);
    let context = context.with_source_text(source.to_owned());
    let ops = match reduce(source) {
        Ok(ops) => ops,
        Err(error) => return RunOutcome::fail(1, format!("reduce: {error}")),
    };
    sync_process_exit_code(&host);
    classify(
        normalize_script_completion(execute_code_with_context(ops.code(), &context)).map(|_| ()),
        host.exit_code(),
    )
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
            Err(error) => Err(error),
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
        (Err(error), None) => RunOutcome::fail(1, error.render()),
    }
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
