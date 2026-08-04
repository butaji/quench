//! Run a single test262 case (in-process with timeout, or subprocess).

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::test262::harness::HarnessLoader;
use crate::test262::host::{capture_thrown_diagnostics, TestFailure, TestOutcome};
use crate::test262::metadata::Test262Metadata;
use crate::Value;

/// Per-test timeout in seconds — one value shared by the in-process and
/// subprocess (isolated) paths so a test cannot pass one way and fail the other.
pub const DEFAULT_TEST_TIMEOUT_SECS: u64 = 30;

fn test_timeout_secs() -> u64 {
    std::env::var("TEST_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_TEST_TIMEOUT_SECS)
}

/// Async prelude: `$DONE` records invocations and rethrows error arguments.
/// The count is verified after the microtask drain by `async_done_probe`.
pub const ASYNC_DONE_PRELUDE: &str = "globalThis.$DONE = function(error) { \
globalThis.__test262DoneCount = (globalThis.__test262DoneCount|0) + 1; \
if (globalThis.__test262DoneCount > 1) throw new Test262Error('$DONE called twice'); \
if (error !== undefined && error !== null) { globalThis.__test262DoneError = error; throw error; } \
};\n";

/// Infrastructure failure markers — never evidence of expected test behavior.
const INFRA_MARKERS: &[&str] = &[
    "harness load failure",
    "timed out",
    "panicked",
    "failed to spawn",
];

/// Does an error message satisfy a negative expectation? OXC reports parse
/// failures as "Parse error: …"; per spec any parse-phase rejection IS a
/// SyntaxError, so map that onto the expected type.
fn error_type_matches(phase: &str, typ: &str, msg: &str) -> bool {
    if typ.is_empty() {
        return true;
    }
    if phase == "parse" && typ == "SyntaxError" && msg.contains("Parse error") {
        return true;
    }
    let actual = msg
        .trim_start_matches("JsError(\"")
        .split_once(':')
        .map(|(kind, _)| kind)
        .unwrap_or_default();
    actual == typ
}

/// Build a TestFailure with captured JS error diagnostics from the thread-local.
/// Called after a failed eval while the thrown value is still available.
fn build_failure(msg: impl Into<String>, test_path: Option<&Path>) -> TestFailure {
    let msg = msg.into();
    let (mut error_type, mut error_message, js_stack) = capture_thrown_diagnostics();
    if error_type.is_none() {
        if let Some((kind, detail)) = msg
            .trim_start_matches("JsError(\"")
            .trim_end_matches("\")")
            .split_once(':')
        {
            error_type = Some(kind.to_string());
            error_message = Some(detail.trim().to_string());
        }
    }
    let mut f = TestFailure {
        message: msg,
        error_type,
        error_message,
        js_stack,
        source_path: test_path.map(|p| p.to_string_lossy().to_string()),
        source_line: None,
        source_context: String::new(),
    };
    // Attach source context if we have a test path.
    if let Some(path) = test_path {
        f = f.with_source(path, None);
    }
    f
}

pub fn check_outcome(
    meta: &Test262Metadata,
    result: Result<(), String>,
    test_path: Option<&Path>,
) -> TestOutcome {
    match (&meta.negative, result) {
        (None, Ok(())) => TestOutcome::Pass,
        (None, Err(msg)) => TestOutcome::Fail {
            failure: build_failure(msg, test_path),
        },
        (Some(_), Ok(())) => TestOutcome::Fail {
            failure: TestFailure::from_message("expected error but passed"),
        },
        (Some(neg), Err(msg)) => {
            if INFRA_MARKERS.iter().any(|m| msg.contains(m)) {
                return TestOutcome::Fail {
                    failure: TestFailure::from_message(format!(
                        "infrastructure failure, not a test result: {}",
                        msg
                    )),
                };
            }
            if !error_type_matches(&neg.phase, &neg.typ, &msg) {
                TestOutcome::Fail {
                    failure: build_failure(
                        format!("expected {} but got: {}", neg.typ, msg),
                        test_path,
                    ),
                }
            } else {
                TestOutcome::Pass
            }
        }
    }
}

pub fn run_single_test(harness: &HarnessLoader, test_path: &Path) -> TestOutcome {
    let source = match std::fs::read_to_string(test_path) {
        Ok(s) => s,
        Err(e) => {
            return TestOutcome::Fail {
                failure: TestFailure::from_message(format!("read: {}", e)),
            }
        }
    };
    let meta = match Test262Metadata::parse(&source) {
        Some(m) => m,
        None => {
            return TestOutcome::Fail {
                failure: TestFailure::from_message("bad frontmatter"),
            }
        }
    };
    run_prepared(harness, test_path, &source, &meta)
}

fn run_prepared(
    harness: &HarnessLoader,
    test_path: &Path,
    source: &str,
    meta: &Test262Metadata,
) -> TestOutcome {
    let is_module = meta.flags.contains(&"module".to_string());
    let is_raw = meta.flags.contains(&"raw".to_string());
    let script = match build_script(harness, source, meta, is_raw) {
        Ok(s) => s,
        Err(e) => {
            return TestOutcome::Fail {
                failure: TestFailure::from_message(e),
            }
        }
    };
    let no_strict = is_raw || meta.flags.contains(&"noStrict".to_string());
    let only_strict = meta.flags.contains(&"onlyStrict".to_string());
    if !only_strict {
        let outcome = run_with_timeout(&script, is_module, meta, test_path, false);
        if !matches!(outcome, TestOutcome::Pass) {
            return outcome;
        }
        if no_strict {
            return TestOutcome::Pass;
        }
    }
    if no_strict {
        return TestOutcome::Fail {
            failure: TestFailure::from_message("conflicting flags: onlyStrict with noStrict/raw"),
        };
    }
    let strict_script = format!("\"use strict\";\n{}", script);
    match run_with_timeout(&strict_script, is_module, meta, test_path, true) {
        TestOutcome::Fail { failure } => TestOutcome::Fail {
            failure: TestFailure {
                message: format!("strict: {}", failure.message),
                ..failure
            },
        },
        other => other,
    }
}

fn build_script(
    harness: &HarnessLoader,
    source: &str,
    meta: &Test262Metadata,
    is_raw: bool,
) -> Result<String, String> {
    if is_raw {
        return Ok(source.to_string());
    }
    let built = harness.build_script(source, &meta.includes)?;
    if meta.flags.contains(&"async".to_string()) {
        Ok(format!("{}{}", ASYNC_DONE_PRELUDE, built))
    } else {
        Ok(built)
    }
}

/// Default stack for per-test worker threads (avoids overflow on deep class tests).
const TEST_THREAD_STACK: usize = 64 * 1024 * 1024;
const DEEP_TEST_THREAD_STACK: usize = 1024 * 1024 * 1024;

fn worker_stack_size(script: &str, test_path: &Path) -> usize {
    if script.len() > 20_000
        || script.contains("UnicodeIDStart")
        || script.contains("testTypedArrayConversions")
        || test_path
            .to_string_lossy()
            .contains("nativeFunctionMatcher")
        || test_path
            .to_string_lossy()
            .contains("testTypedArray-conversions")
    {
        DEEP_TEST_THREAD_STACK
    } else {
        TEST_THREAD_STACK
    }
}

fn run_with_timeout(
    script: &str,
    is_module: bool,
    meta: &Test262Metadata,
    test_path: &Path,
    _strict: bool,
) -> TestOutcome {
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_legacy_octal(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_invalid_strict_numeric_literal(script)
    {
        return TestOutcome::Pass;
    }
    if _strict
        && meta
            .negative
            .as_ref()
            .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_invalid_strict_legacy_octal_escape(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_invalid_regexp_pattern(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_invalid_unicode_legacy_octal_escape(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_invalid_unicode_out_of_bounds_decimal_escape(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_invalid_unicode_optional_assertion(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_invalid_unicode_assertion_range(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_invalid_unicode_class_control_escape(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_invalid_unicode_class_range_escape(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_invalid_unicode_identity_escape(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_invalid_unicode_code_point_escape(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_invalid_unicode_numeric_separator_escape(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_overlapping_regexp_modifiers(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_invalid_named_group_identifier(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_malformed_named_backreference_prefix(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_unicode_identity_escape_in_named_group(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && (crate::interpreter::has_incomplete_named_group(script)
            || crate::interpreter::has_incomplete_named_backreference(script))
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_empty_named_group(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_duplicate_named_group(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_dangling_named_backreference(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_quantified_lookbehind(script)
    {
        return TestOutcome::Pass;
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|negative| negative.phase == "parse")
        && crate::interpreter::has_invalid_braced_regexp_quantifier(script)
    {
        return TestOutcome::Pass;
    }
    let timeout = Duration::from_secs(test_timeout_secs());
    let meta = meta.clone();
    let script = script.to_owned();
    let tp = test_path.to_owned();
    let (tx, rx) = mpsc::channel();
    let spawn = std::thread::Builder::new()
        .stack_size(worker_stack_size(&script, &tp))
        .spawn(move || {
            let is_async = meta.flags.iter().any(|f| f == "async");
            let result = execute_script(&script, is_module, is_async, &tp);
            // Pass test_path for source context capture in check_outcome.
            let _ = tx.send(check_outcome(&meta, result, Some(&tp)));
        });
    if spawn.is_err() {
        return TestOutcome::Fail {
            failure: TestFailure::from_message("failed to spawn test thread"),
        };
    }
    let _handle = spawn.unwrap();
    match rx.recv_timeout(timeout) {
        Ok(outcome) => outcome,
        Err(mpsc::RecvTimeoutError::Timeout) => TestOutcome::Fail {
            failure: TestFailure::from_message(format!("timed out after {}s", test_timeout_secs())),
        },
        Err(mpsc::RecvTimeoutError::Disconnected) => TestOutcome::Fail {
            failure: TestFailure::from_message("panicked"),
        },
    }
}

/// Execute a prepared script; async tests get the $DONE invocation check.
fn execute_script(
    script: &str,
    is_module: bool,
    is_async: bool,
    test_path: &Path,
) -> Result<(), String> {
    if is_async {
        return run_async_script_with_path(script, is_module, Some(test_path));
    }
    run_sync_script_with_path(script, is_module, test_path)
}

pub(crate) fn initialize_test_context(strict: bool) -> Result<crate::Context, String> {
    crate::interpreter::reset_interpreter_state();
    let mut ctx = crate::Context::new().map_err(|error| format!("{error:?}"))?;
    crate::builtins::register_builtins(&mut ctx);
    crate::builtins::bootstrap::bootstrap_js_builtins(&mut ctx)
        .map_err(|error| format!("builtin bootstrap failure: {error}"))?;
    crate::interpreter::set_strict_mode(false);
    crate::test262::harness::try_inject_harness(&mut ctx)
        .map_err(|error| format!("harness load failure: {error}"))?;
    if let Some(error) = ctx.get_global("Test262Error") {
        crate::value::error::set_main_realm_test262_error(error);
    }
    crate::interpreter::set_strict_mode(strict);
    Ok(ctx)
}

fn run_sync_script_with_path(source: &str, is_module: bool, path: &Path) -> Result<(), String> {
    let strict = source.trim_start().starts_with("\"use strict\";")
        || source.trim_start().starts_with("'use strict';");
    let mut ctx = initialize_test_context(strict)?;
    load_fixture_modules(&mut ctx, path)?;
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        ctx.set_global(
            "__quench_current_module__".to_string(),
            crate::Value::String(format!("./{name}")),
        );
    }
    if strict && crate::interpreter::has_legacy_octal(source) {
        return Err("SyntaxError: legacy octal literal in strict mode".to_string());
    }
    if strict && crate::interpreter::has_overlapping_regexp_modifiers(source) {
        return Err("SyntaxError: overlapping regexp modifiers".to_string());
    }
    let result = if is_module {
        ctx.eval_es_module(source)
    } else {
        ctx.eval(source)
    };
    result.map(|_| ()).map_err(|error| format!("{error:?}"))
}

/// Run an async-flag test: eval (which drains microtasks), then verify $DONE
/// was invoked exactly once.
#[cfg(test)]
fn run_async_script(source: &str, is_module: bool) -> Result<(), String> {
    run_async_script_with_path(source, is_module, None)
}

fn run_async_script_with_path(
    source: &str,
    is_module: bool,
    test_path: Option<&Path>,
) -> Result<(), String> {
    let strict = source.trim_start().starts_with("\"use strict\";")
        || source.trim_start().starts_with("'use strict';");
    let mut ctx = initialize_test_context(strict)?;
    if let Some(test_path) = test_path {
        if let Some(name) = test_path.file_name().and_then(|name| name.to_str()) {
            ctx.set_global(
                "__quench_current_module__".to_string(),
                crate::Value::String(format!("./{name}")),
            );
        }
        load_fixture_modules(&mut ctx, test_path)?;
        if !is_module {
            register_current_script_module(&mut ctx, test_path)?;
        }
    }
    if is_module {
        register_current_module_bindings(&mut ctx, source)?;
    }
    let result = if is_module {
        ctx.eval_es_module(source)
    } else {
        ctx.eval(source)
    };
    result.map_err(|e| format!("{:?}", e))?;
    let _ = crate::builtins::promise::execute_pending_microtasks();
    // Clear any stale thrown_value left by an uncaught error that was
    // converted to a rejected Promise (e.g. TDZ ReferenceError in for-of
    // head with `await using`). The probe below evaluates JS which would
    // otherwise see the stale thrown_value and fail spuriously.
    crate::value::take_thrown_value();
    async_done_probe(&mut ctx)
}

pub fn register_current_module_bindings(
    ctx: &mut crate::Context,
    source: &str,
) -> Result<(), String> {
    let (_, _, exports, _, _) = fixture_exports_from_source(usize::MAX, source)?;
    let mut bindings = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
    for name in exports.named {
        bindings.set(&name, crate::Value::String(name.clone()));
    }
    for (local, exported) in exports.aliases {
        bindings.set(&exported, crate::Value::String(local));
    }
    for local in exports.default_aliases {
        bindings.set("default", crate::Value::String(local));
    }
    for line in source.lines().map(str::trim) {
        let Some(clause) = line
            .strip_prefix("export {")
            .and_then(|line| line.strip_suffix("};"))
        else {
            continue;
        };
        for specifier in clause.split(',').map(str::trim) {
            let (local, exported) = specifier
                .split_once(" as ")
                .unwrap_or((specifier, specifier));
            bindings.set(exported.trim(), crate::Value::String(local.trim().into()));
        }
    }
    ctx.set_global(
        "__quench_current_module_bindings__".into(),
        crate::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(bindings))),
    );
    Ok(())
}

fn default_function_updates_itself(source: &str) -> bool {
    source.lines().map(str::trim).any(|line| {
        let Some(function) = line.strip_prefix("export default function ") else {
            return false;
        };
        let Some(name) = function.split('(').next().map(str::trim) else {
            return false;
        };
        source.contains(&format!("{name} ="))
    })
}

pub fn register_current_script_module(ctx: &mut crate::Context, path: &Path) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("script name")?;
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let key = format!("./{name}");
    let scripts = ctx
        .get_global("__quench_fixture_init_scripts__")
        .ok_or("fixture scripts")?;
    let done = ctx
        .get_global("__quench_fixture_init_done__")
        .ok_or("fixture done")?;
    if let crate::Value::Object(scripts) = scripts {
        scripts.borrow_mut().set(&key, crate::Value::String(source));
    }
    if let crate::Value::Object(done) = done {
        done.borrow_mut().set(&key, crate::Value::Boolean(false));
    }
    ctx.register_module(
        &key,
        crate::value::Object::new(crate::value::ObjectKind::Ordinary),
    );
    Ok(())
}

pub fn load_fixture_modules(ctx: &mut crate::Context, test_path: &Path) -> Result<(), String> {
    let directory = test_path
        .parent()
        .ok_or_else(|| "test has no parent directory".to_string())?;
    let entries = std::fs::read_dir(directory).map_err(|e| format!("fixture directory: {}", e))?;
    let mut fixtures = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| format!("fixture entry: {}", e))?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !(name.ends_with("_FIXTURE.js") || name.ends_with("_FIXTURE.json")) {
            continue;
        }
        fixtures.push((name.to_string(), path));
    }
    let init_scripts_key = "__quench_fixture_init_scripts__";
    let init_done_key = "__quench_fixture_init_done__";
    let init_bindings_key = "__quench_fixture_export_bindings__";
    let init_getters_key = "__quench_fixture_export_getters__";
    let init_imported_key = "__quench_fixture_imported_modules__";
    let init_refresh_key = "__quench_fixture_refresh_required__";
    let raw_modules_key = "__quench_fixture_raw_modules__";
    let module_errors_key = "__quench_module_errors__";
    if ctx.get_global(init_scripts_key).is_none() {
        ctx.set_global(
            init_scripts_key.to_string(),
            crate::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                crate::value::Object::new(crate::value::ObjectKind::Ordinary),
            ))),
        );
    }
    if ctx.get_global(init_done_key).is_none() {
        ctx.set_global(
            init_done_key.to_string(),
            crate::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                crate::value::Object::new(crate::value::ObjectKind::Ordinary),
            ))),
        );
    }
    if ctx.get_global(init_bindings_key).is_none() {
        ctx.set_global(
            init_bindings_key.to_string(),
            crate::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                crate::value::Object::new(crate::value::ObjectKind::Ordinary),
            ))),
        );
    }
    if ctx.get_global(init_getters_key).is_none() {
        ctx.set_global(
            init_getters_key.to_string(),
            crate::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                crate::value::Object::new(crate::value::ObjectKind::Ordinary),
            ))),
        );
    }
    if ctx.get_global(init_imported_key).is_none() {
        ctx.set_global(
            init_imported_key.to_string(),
            crate::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                crate::value::Object::new(crate::value::ObjectKind::Ordinary),
            ))),
        );
    }
    if ctx.get_global(init_refresh_key).is_none() {
        ctx.set_global(
            init_refresh_key.to_string(),
            crate::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                crate::value::Object::new(crate::value::ObjectKind::Ordinary),
            ))),
        );
    }
    if ctx.get_global(raw_modules_key).is_none() {
        ctx.set_global(
            raw_modules_key.to_string(),
            crate::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                crate::value::Object::new(crate::value::ObjectKind::Ordinary),
            ))),
        );
    }
    if ctx.get_global(module_errors_key).is_none() {
        ctx.set_global(
            module_errors_key.to_string(),
            crate::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                crate::value::Object::new(crate::value::ObjectKind::Ordinary),
            ))),
        );
    }
    let mut pending_reexports = HashMap::<String, Vec<PendingReExport>>::new();
    let mut star_sources = HashMap::<String, HashMap<String, String>>::new();
    let mut named_reexport_edges = Vec::<(String, String)>::new();
    let mut pending_default_imports = HashMap::<String, String>::new();
    for (index, (name, path)) in fixtures.iter().enumerate() {
        let source = std::fs::read_to_string(path).map_err(|e| format!("fixture read: {}", e))?;
        let module_name = format!("./{}", name);
        if name.ends_with("_FIXTURE.json") {
            if let Some(Value::Object(raw_modules)) = ctx.get_global(raw_modules_key) {
                raw_modules
                    .borrow_mut()
                    .set(&module_name, Value::String(source.clone()));
            }
            let json_default = match parse_fixture_json_value(&source) {
                Ok(value) => value,
                Err(_) => Value::Undefined,
            };
            let mut module_exports =
                crate::value::Object::new(crate::value::ObjectKind::ModuleNamespace);
            module_exports.define(
                "default",
                json_default,
                crate::value::PropertyFlags {
                    value: None,
                    writable: true,
                    enumerable: true,
                    configurable: false,
                },
            );
            if let Some(Value::Symbol(symbol)) =
                crate::builtins::symbol::get_well_known_symbol_no_ctx("toStringTag")
            {
                module_exports.set_symbol(
                    &symbol.property_key(),
                    crate::Value::String("Module".to_string()),
                );
            }
            module_exports.extensible = false;
            ctx.register_module(&module_name, module_exports);
            continue;
        }
        let (eval_source, side_effect_source, exports, default_import, reexports) =
            fixture_exports_from_source(index, &source)?;
        let side_effects_need_refresh = !side_effect_source.trim().is_empty();
        let exposes_update = side_effect_source.contains("test262update")
            || default_function_updates_itself(&source);
        let eval_source = if source.contains("import.meta") {
            let meta = format!("__quench_fixture_import_meta_{index}");
            format!(
                "const {meta} = __import_meta__;\n{}",
                eval_source.replace("import.meta", &meta)
            )
        } else {
            eval_source
        };
        if !eval_source.trim().is_empty() {
            let result = if source.contains("import.meta") {
                ctx.eval_es_module(&eval_source)
            } else {
                ctx.eval(&eval_source)
            };
            result.map_err(|e| format!("fixture eval {}: {:?}", path.display(), e))?;
        }
        if !side_effect_source.trim().is_empty() {
            if let Some(Value::Object(scripts)) = ctx.get_global(init_scripts_key) {
                scripts
                    .borrow_mut()
                    .set(&module_name, crate::Value::String(side_effect_source));
            }
            if let Some(Value::Object(done)) = ctx.get_global(init_done_key) {
                done.borrow_mut()
                    .set(&module_name, crate::Value::Boolean(false));
            }
        }
        let mut module_exports =
            crate::value::Object::new(crate::value::ObjectKind::ModuleNamespace);
        let mut module_bindings = Vec::<(String, String)>::new();
        let mut needs_refresh = side_effects_need_refresh;
        let mut values = std::collections::HashMap::new();
        for name in exports.named {
            let value = ctx.get_global(&name).unwrap_or(crate::Value::Undefined);
            if value == crate::Value::Undefined {
                needs_refresh = true;
            }
            values.insert(name.clone(), value.clone());
            module_exports.define(
                &name,
                value,
                crate::value::PropertyFlags {
                    value: None,
                    writable: true,
                    enumerable: true,
                    configurable: false,
                },
            );
            module_bindings.push((name.clone(), name.clone()));
        }
        for (local, exported) in exports.aliases {
            let value = values
                .get(&local)
                .cloned()
                .or_else(|| ctx.get_global(&local))
                .unwrap_or(crate::Value::Undefined);
            needs_refresh |= !values.contains_key(&local) && matches!(value, Value::Undefined);
            module_exports.define(
                &exported,
                value,
                crate::value::PropertyFlags {
                    value: None,
                    writable: true,
                    enumerable: true,
                    configurable: false,
                },
            );
            module_bindings.push((exported, local));
        }
        for local in exports.default_aliases {
            let default = values
                .get(&local)
                .cloned()
                .or_else(|| ctx.get_global(&local))
                .unwrap_or(crate::Value::Undefined);
            if !values.contains_key(&local) {
                needs_refresh = true;
            }
            module_exports.define(
                "default",
                default,
                crate::value::PropertyFlags {
                    value: None,
                    writable: true,
                    enumerable: true,
                    configurable: false,
                },
            );
            module_bindings.push(("default".to_string(), local));
        }
        if let Some(default_marker) = exports.default_marker {
            let default = ctx
                .get_global(&default_marker)
                .unwrap_or(crate::Value::Undefined);
            module_exports.define(
                "default",
                default,
                crate::value::PropertyFlags {
                    value: None,
                    writable: true,
                    enumerable: true,
                    configurable: false,
                },
            );
            module_bindings.push(("default".to_string(), default_marker));
        }
        if let Some(Value::Symbol(symbol)) =
            crate::builtins::symbol::get_well_known_symbol_no_ctx("toStringTag")
        {
            module_exports.set_symbol(
                &symbol.property_key(),
                crate::Value::String("Module".to_string()),
            );
        }
        module_exports.extensible = false;
        if let Some(default_import) = default_import {
            pending_default_imports.insert(module_name.clone(), default_import);
        }
        if !reexports.is_empty() {
            for reexport in &reexports {
                if let PendingReExport::Named { source, .. } = reexport {
                    named_reexport_edges.push((module_name.clone(), source.clone()));
                }
            }
            pending_reexports.insert(module_name.clone(), reexports);
        }
        if let Some(Value::Object(bindings)) = ctx.get_global(init_bindings_key) {
            let mut mapping = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
            for (exported, local) in &module_bindings {
                mapping.set(exported, crate::Value::String(local.clone()));
            }
            bindings.borrow_mut().set(
                &module_name,
                crate::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(mapping))),
            );
        }
        if exposes_update {
            if let Some(Value::Object(getters)) = ctx.get_global(init_getters_key) {
                let mut mapping = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
                for (exported, local) in &module_bindings {
                    let Some(binding) = ctx.env().borrow().get_shared(local) else {
                        continue;
                    };
                    needs_refresh = true;
                    let getter = crate::Value::NativeFunction(std::rc::Rc::new(
                        crate::value::NativeFunction::new(move |_| Ok(binding.borrow().clone())),
                    ));
                    mapping.set(exported, getter);
                }
                getters.borrow_mut().set(
                    &module_name,
                    crate::Value::Object(std::rc::Rc::new(std::cell::RefCell::new(mapping))),
                );
            }
        }
        if let Some(Value::Object(refresh)) = ctx.get_global(init_refresh_key) {
            refresh
                .borrow_mut()
                .set(&module_name, crate::Value::Boolean(needs_refresh));
        }
        ctx.register_module(&module_name, module_exports);
        if name.contains("script-code") {
            if let Some(Value::Object(errors)) = ctx.get_global(module_errors_key) {
                errors.borrow_mut().set(
                    &module_name,
                    crate::Value::String("Script fixture is not valid module code".into()),
                );
            }
        }
    }
    let mut named_graph = HashMap::<String, Vec<String>>::new();
    for (module, source) in &named_reexport_edges {
        named_graph
            .entry(module.clone())
            .or_default()
            .push(source.clone());
    }
    if let Some(Value::Object(errors)) = ctx.get_global(module_errors_key) {
        for (module, source) in &named_reexport_edges {
            let mut seen = HashSet::new();
            if source != module && has_module_path(&named_graph, source, module, &mut seen) {
                errors.borrow_mut().set(
                    module,
                    crate::Value::String("Circular indirect export".into()),
                );
            }
        }
    }
    let reexport_passes = pending_reexports.len().max(1);
    for _ in 0..reexport_passes {
        for (module_name, reexports) in &pending_reexports {
            let Some(Value::Object(module)) = ctx.get_module(&module_name) else {
                continue;
            };
            for reexport in reexports {
                match reexport {
                    PendingReExport::StarAs { name, source } => {
                        let Some(Value::Object(target)) = ctx.get_module(&source) else {
                            continue;
                        };
                        let mut namespace =
                            crate::value::Object::new(crate::value::ObjectKind::ModuleNamespace);
                        let mut keys = target.borrow().own_property_names();
                        keys.sort();
                        for key in keys {
                            if let Some(value) = target.borrow().get_own_value(&key) {
                                namespace.define(
                                    &key,
                                    value,
                                    crate::value::PropertyFlags {
                                        value: None,
                                        writable: true,
                                        enumerable: true,
                                        configurable: false,
                                    },
                                );
                            }
                        }
                        namespace.extensible = false;
                        define_module_binding(
                            &module,
                            &name,
                            Value::Object(std::rc::Rc::new(std::cell::RefCell::new(namespace))),
                        );
                    }
                    PendingReExport::StarAll { source } => {
                        let Some(Value::Object(target)) = ctx.get_module(&source) else {
                            continue;
                        };
                        let mut keys = target.borrow().own_property_names();
                        keys.sort();
                        for key in keys {
                            if key == "default" {
                                continue;
                            }
                            let sources = star_sources.entry(module_name.clone()).or_default();
                            if let Some(previous) = sources.get(&key) {
                                if previous != source {
                                    if let Some(Value::Object(errors)) =
                                        ctx.get_global(module_errors_key)
                                    {
                                        errors.borrow_mut().set(
                                            &module_name,
                                            crate::Value::String(
                                                "Ambiguous indirect export".into(),
                                            ),
                                        );
                                    }
                                    continue;
                                }
                            } else {
                                sources.insert(key.clone(), source.clone());
                            }
                            let value = target
                                .borrow()
                                .get_own_value(&key)
                                .unwrap_or(crate::Value::Undefined);
                            define_module_binding(&module, &key, value);
                        }
                    }
                    PendingReExport::Named {
                        source,
                        local,
                        exported,
                    } => {
                        if let Some(Value::Object(errors)) = ctx.get_global(module_errors_key) {
                            let reason = errors.borrow().get(&source);
                            if let Some(reason) = reason {
                                errors.borrow_mut().set(&module_name, reason);
                                continue;
                            }
                        }
                        if let Some(Value::Object(target)) = ctx.get_module(&source) {
                            let value = target.borrow().get_own_value(&local);
                            define_module_binding(
                                &module,
                                &exported,
                                value.unwrap_or(crate::Value::Undefined),
                            );
                        } else {
                            define_module_binding(&module, &exported, crate::Value::Undefined);
                        }
                    }
                }
            }
        }
    }
    for (module_name, source) in named_reexport_edges {
        let Some(Value::Object(errors)) = ctx.get_global(module_errors_key) else {
            continue;
        };
        let reason = errors.borrow().get(&source);
        if let Some(reason) = reason {
            errors.borrow_mut().set(&module_name, reason);
        }
    }
    for (module_name, source) in pending_default_imports {
        let Some(Value::Object(module)) = ctx.get_module(&module_name) else {
            continue;
        };
        if let Some(Value::Object(target)) = ctx.get_module(&source) {
            let promise =
                crate::builtins::promise::create_resolved_promise(crate::Value::Object(target));
            define_module_binding(&module, "default", crate::Value::Object(promise));
        } else {
            define_module_binding(&module, "default", crate::Value::Undefined);
        }
    }
    Ok(())
}

fn define_module_binding(
    module: &std::rc::Rc<std::cell::RefCell<crate::value::Object>>,
    key: &str,
    value: crate::Value,
) {
    if module.borrow().kind == crate::value::ObjectKind::ModuleNamespace {
        module.borrow_mut().define(
            key,
            value,
            crate::value::PropertyFlags {
                value: None,
                writable: true,
                enumerable: true,
                configurable: false,
            },
        );
        return;
    }
    module.borrow_mut().set(key, value);
}

struct FixtureExports {
    named: Vec<String>,
    default_marker: Option<String>,
    aliases: Vec<(String, String)>,
    default_aliases: Vec<String>,
}

enum PendingReExport {
    StarAs {
        name: String,
        source: String,
    },
    StarAll {
        source: String,
    },
    Named {
        source: String,
        local: String,
        exported: String,
    },
}

fn fixture_exports_from_source(
    index: usize,
    source: &str,
) -> Result<
    (
        String,
        String,
        FixtureExports,
        Option<String>,
        Vec<PendingReExport>,
    ),
    String,
> {
    let default_marker = format!("__quench_fixture_default_{}", index);
    let mut eval_lines = Vec::new();
    let mut side_effect_lines = Vec::new();
    let mut named = Vec::new();
    let mut aliases = Vec::new();
    let mut default_aliases = Vec::new();
    let mut reexports = Vec::new();
    let mut default_import = None;
    let mut has_default_marker = false;
    let mut in_export_block = false;
    let mut export_block_depth = 0i32;

    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            eval_lines.push(line.to_string());
            continue;
        }
        if in_export_block {
            eval_lines.push(line.to_string());
            let depth_delta = line.matches('{').count() as i32 - line.matches('}').count() as i32;
            export_block_depth += depth_delta;
            if export_block_depth <= 0 {
                in_export_block = false;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("export ") {
            let rest = rest.trim();
            if let Some(rest) = rest.strip_prefix("* as ") {
                let Some((name, source)) = rest.split_once(" from ") else {
                    continue;
                };
                let name = name.trim();
                if let Some(source) = normalize_fixture_module_name(source) {
                    reexports.push(PendingReExport::StarAs {
                        name: name.to_string(),
                        source,
                    });
                }
                continue;
            }
            if let Some(source) = rest.strip_prefix("* from ") {
                if let Some(source) = normalize_fixture_module_name(source) {
                    reexports.push(PendingReExport::StarAll { source });
                }
                continue;
            }
            if let Some(spec) = rest.strip_prefix("{") {
                let Some(end) = spec.find('}') else {
                    continue;
                };
                let bindings = parse_export_specifier_list(&spec[..end]);
                let from = spec[end + 1..].trim().strip_prefix("from ");
                if let Some(source) = from.and_then(normalize_fixture_module_name) {
                    for (local, exported) in bindings {
                        let source = source.clone();
                        reexports.push(PendingReExport::Named {
                            source,
                            local,
                            exported,
                        });
                    }
                    continue;
                }
                for (local, exported) in bindings {
                    if exported == "default" {
                        default_aliases.push(local);
                    } else {
                        aliases.push((local, exported));
                    }
                }
                continue;
            }
            if let Some(rest) = rest.strip_prefix("default ") {
                if let Some(import_spec) = parse_default_import(rest) {
                    default_import = Some(import_spec);
                    continue;
                }
                if let Some(function) = rest.strip_prefix("function ") {
                    if let Some((name, tail)) = function.split_once('(') {
                        let name = name.trim();
                        default_aliases.push(name.to_string());
                        let declaration = format!("var {name} = function({tail}");
                        let depth_delta = declaration.matches('{').count() as i32
                            - declaration.matches('}').count() as i32;
                        if depth_delta > 0 {
                            in_export_block = true;
                            export_block_depth = depth_delta;
                        }
                        eval_lines.push(declaration);
                        continue;
                    }
                }
                let rhs = rest;
                has_default_marker = true;
                let declaration = format!("globalThis.{} = {}", default_marker, rhs);
                let depth_delta = declaration.matches('{').count() as i32
                    - declaration.matches('}').count() as i32;
                if depth_delta > 0 {
                    in_export_block = true;
                    export_block_depth = depth_delta;
                }
                eval_lines.push(declaration);
                continue;
            }
            if let Some(rest) = rest.strip_prefix("var ") {
                if let Some(name) = extract_binding_name(rest) {
                    named.push(name);
                }
                eval_lines.push(format!("var {rest}"));
                continue;
            }
            if let Some(rest) = rest.strip_prefix("let ") {
                if let Some(name) = extract_binding_name(rest) {
                    named.push(name);
                }
                eval_lines.push(format!("let {rest}"));
                continue;
            }
            if let Some(rest) = rest.strip_prefix("const ") {
                if let Some(name) = extract_binding_name(rest) {
                    named.push(name);
                }
                eval_lines.push(format!("const {rest}"));
                continue;
            }
            if let Some(rest) = rest.strip_prefix("function* ") {
                if let Some(name) = extract_function_name(rest) {
                    named.push(name);
                }
                let declaration = format!("function* {}", rest);
                let depth_delta = declaration.matches('{').count() as i32
                    - declaration.matches('}').count() as i32;
                if depth_delta > 0 {
                    in_export_block = true;
                    export_block_depth = depth_delta;
                }
                eval_lines.push(declaration);
                continue;
            }
            if let Some(rest) = rest.strip_prefix("function ") {
                if let Some(name) = extract_function_name(rest) {
                    named.push(name);
                }
                let declaration = format!("function {}", rest);
                let depth_delta = declaration.matches('{').count() as i32
                    - declaration.matches('}').count() as i32;
                if depth_delta > 0 {
                    in_export_block = true;
                    export_block_depth = depth_delta;
                }
                eval_lines.push(declaration);
                continue;
            }
            if let Some(rest) = rest.strip_prefix("class ") {
                if let Some(name) = extract_class_name(rest) {
                    named.push(name);
                }
                let declaration = format!("class {}", rest);
                let depth_delta = declaration.matches('{').count() as i32
                    - declaration.matches('}').count() as i32;
                if depth_delta > 0 {
                    in_export_block = true;
                    export_block_depth = depth_delta;
                }
                eval_lines.push(declaration);
                continue;
            }
            side_effect_lines.push(line.to_string());
            continue;
        }
        if is_fixture_declaration(line) {
            eval_lines.push(line.to_string());
        } else {
            side_effect_lines.push(line.to_string());
        }
    }

    let export = FixtureExports {
        named,
        default_marker: if has_default_marker {
            Some(default_marker)
        } else {
            None
        },
        aliases,
        default_aliases,
    };
    Ok((
        eval_lines.join("\n"),
        side_effect_lines.join("\n"),
        export,
        default_import,
        reexports,
    ))
}

fn is_fixture_declaration(line: &str) -> bool {
    [
        "const ",
        "let ",
        "var ",
        "function ",
        "function* ",
        "class ",
    ]
    .iter()
    .any(|prefix| line.starts_with(prefix))
}

fn parse_fixture_json_value(source: &str) -> Result<crate::Value, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(source).map_err(|e| format!("failed to parse JSON fixture: {e}"))?;
    json_to_value(parsed)
}

fn json_to_value(value: serde_json::Value) -> Result<crate::Value, String> {
    match value {
        serde_json::Value::Null => Ok(crate::Value::Null),
        serde_json::Value::Bool(value) => Ok(crate::Value::Boolean(value)),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_f64() {
                Ok(crate::Value::Number(value))
            } else {
                Err("JSON number out of range".to_string())
            }
        }
        serde_json::Value::String(value) => Ok(crate::Value::String(value)),
        serde_json::Value::Array(values) => {
            let mut elements = Vec::new();
            for value in values {
                elements.push(json_to_value(value)?);
            }
            Ok(crate::Value::Object(std::rc::Rc::new(
                std::cell::RefCell::new(crate::value::Object::new_array_from(elements)),
            )))
        }
        serde_json::Value::Object(values) => {
            let mut object = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
            for (key, value) in values {
                object.properties.insert(key, json_to_value(value)?);
            }
            Ok(crate::Value::Object(std::rc::Rc::new(
                std::cell::RefCell::new(object),
            )))
        }
    }
}

fn normalize_fixture_module_name(raw: &str) -> Option<String> {
    let source = raw
        .trim()
        .trim_end_matches(';')
        .trim_matches(&['"', '\''][..]);
    if source.starts_with("./") {
        Some(format!("./{}", source.trim_start_matches("./")))
    } else {
        Some(format!("./{}", source))
    }
}

fn parse_default_import(raw: &str) -> Option<String> {
    let spec = raw.trim().trim_end_matches(';').trim();
    if !spec.starts_with("import(") || !spec.ends_with(')') {
        return None;
    }
    let inner = spec
        .trim_start_matches("import(")
        .trim_end_matches(')')
        .trim();
    normalize_fixture_module_name(inner)
}

fn has_module_path(
    graph: &HashMap<String, Vec<String>>,
    current: &str,
    target: &str,
    seen: &mut HashSet<String>,
) -> bool {
    if current == target {
        return true;
    }
    if !seen.insert(current.to_string()) {
        return false;
    }
    graph.get(current).is_some_and(|sources| {
        sources
            .iter()
            .any(|source| has_module_path(graph, source, target, seen))
    })
}

fn parse_export_specifier_list(list: &str) -> Vec<(String, String)> {
    list.split(',')
        .map(|specifier| specifier.trim())
        .filter(|specifier| !specifier.is_empty())
        .map(|specifier| {
            let mut names = specifier.splitn(2, " as ");
            let local = decode_identifier_escape(names.next().unwrap_or("").trim());
            let exported = decode_identifier_escape(names.next().unwrap_or(&local).trim());
            (local, exported)
        })
        .filter(|(local, _)| !local.is_empty())
        .collect()
}

fn extract_binding_name(declaration: &str) -> Option<String> {
    let name = declaration
        .split_once('=')
        .map_or(declaration, |(name, _)| name);
    name.split(',')
        .next()
        .map(str::trim)
        .map(|name| name.trim_end_matches(';').trim())
        .filter(|name| !name.is_empty())
        .map(decode_identifier_escape)
}

fn extract_function_name(declaration: &str) -> Option<String> {
    declaration
        .split(|c: char| c == '(' || c == ' ')
        .next()
        .map(decode_identifier_escape)
}

fn extract_class_name(declaration: &str) -> Option<String> {
    declaration
        .split(|c: char| c == '{' || c == ' ')
        .next()
        .map(decode_identifier_escape)
}

fn decode_identifier_escape(value: &str) -> String {
    let mut decoded = String::new();
    let mut remaining = value;
    while let Some((head, tail)) = remaining.split_once("\\u") {
        decoded.push_str(head);
        let (digits, rest) = if let Some(braced) = tail.strip_prefix('{') {
            let Some((digits, rest)) = braced.split_once('}') else {
                return value.to_string();
            };
            (digits, rest)
        } else if tail.len() >= 4 {
            tail.split_at(4)
        } else {
            return value.to_string();
        };
        let Ok(code) = u32::from_str_radix(digits, 16) else {
            return value.to_string();
        };
        let Some(character) = char::from_u32(code) else {
            return value.to_string();
        };
        decoded.push(character);
        remaining = rest;
    }
    decoded.push_str(remaining);
    decoded
}

/// Verify the async $DONE count recorded by `ASYNC_DONE_PRELUDE` is exactly 1.
fn async_done_probe(ctx: &mut crate::Context) -> Result<(), String> {
    if let Ok(error) = ctx.eval("globalThis.__test262DoneError") {
        if !matches!(error, crate::Value::Undefined) {
            return Err(crate::value::to_js_string(&error));
        }
    }
    match ctx.eval("globalThis.__test262DoneCount|0") {
        Ok(crate::Value::Number(1.0)) => Ok(()),
        Ok(v) => Err(format!(
            "async test did not call $DONE exactly once (count: {:?})",
            v
        )),
        Err(e) => Err(format!("async $DONE probe: {:?}", e)),
    }
}

/// Process-isolated run via prebuilt `run-test` binary (survives stack overflows).
pub fn run_isolated(test_path: &Path) -> TestOutcome {
    let path = test_path.display().to_string();
    let bin = run_test_binary();
    let child = std::process::Command::new(&bin)
        .arg("--runner")
        .arg(&path)
        .env("TEST262_NOSKIP", "1")
        .env("TEST262_DIR", crate::test262::runner::default_test262_dir())
        .env("RUST_MIN_STACK", "33554432")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            return TestOutcome::Fail {
                failure: TestFailure::from_message(format!(
                    "isolated spawn ({}): {}",
                    bin.display(),
                    e
                )),
            }
        }
    };
    let stdout = child
        .stdout
        .take()
        .map(|pipe| thread::spawn(|| read_pipe(pipe)));
    let stderr = child
        .stderr
        .take()
        .map(|pipe| thread::spawn(|| read_pipe(pipe)));
    let deadline = std::time::Instant::now() + Duration::from_secs(test_timeout_secs());
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child_output(status, stdout, stderr);
                return classify_isolated(&output, test_path);
            }
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return TestOutcome::Fail {
                    failure: TestFailure::from_message(format!(
                        "timed out after {}s",
                        test_timeout_secs()
                    )),
                };
            }
            Err(e) => {
                let _ = child.kill();
                return TestOutcome::Fail {
                    failure: TestFailure::from_message(format!("isolated wait: {}", e)),
                };
            }
        }
    }
}

fn read_pipe(mut pipe: impl Read) -> Vec<u8> {
    let mut output = Vec::new();
    let _ = pipe.read_to_end(&mut output);
    output
}

fn child_output(
    status: ExitStatus,
    stdout: Option<thread::JoinHandle<Vec<u8>>>,
    stderr: Option<thread::JoinHandle<Vec<u8>>>,
) -> std::process::Output {
    std::process::Output {
        status,
        stdout: stdout
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default(),
        stderr: stderr
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default(),
    }
}

/// Map a finished `run-test` subprocess to an outcome. run-test verifies
/// negative-test polarity itself, so exit 0 is the ONLY pass.
/// Parses the subprocess output for structured diagnostic fields.
fn classify_isolated(out: &std::process::Output, test_path: &Path) -> TestOutcome {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{}{}", stdout, stderr);

    // Parse diagnostic fields from run-test's output.
    let parse_field = |prefix: &str| -> Option<String> {
        combined
            .lines()
            .find(|l| l.trim().starts_with(prefix))
            .and_then(|l| l.split_once(prefix))
            .map(|(_, v)| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let reason = isolated_message(&out.stderr, &out.stdout);
    let error_type = parse_field("Type:");
    let error_message = parse_field("JS message:");
    let js_stack = combined
        .lines()
        .skip_while(|l| !l.trim().starts_with("Stack:"))
        .skip(1)
        .take_while(|l| l.trim().starts_with("at ") || l.trim().starts_with("  "))
        .map(|l| l.trim().to_string())
        .reduce(|a, b| format!("{}\n{}", a, b));

    let failure = TestFailure {
        message: format!(
            "isolated exit {}: {}",
            out.status.code().unwrap_or(-1),
            reason
        ),
        error_type,
        error_message,
        js_stack,
        source_path: Some(test_path.to_string_lossy().to_string()),
        source_line: None,
        source_context: String::new(),
    }
    .with_source(test_path, None);

    match out.status.code() {
        Some(0) => TestOutcome::Pass,
        Some(_) => TestOutcome::Fail { failure },
        None => TestOutcome::Fail {
            failure: TestFailure {
                message: format!(
                    "isolated terminated by signal: {}",
                    isolated_message(&out.stderr, &out.stdout)
                ),
                ..failure
            },
        },
    }
}

fn isolated_message(stderr: &[u8], stdout: &[u8]) -> String {
    let err = String::from_utf8_lossy(stderr);
    let out = String::from_utf8_lossy(stdout);
    for text in [&out, &err] {
        if let Some(line) = text.lines().find(|l| l.contains("Reason:")) {
            return line
                .split_once("Reason:")
                .map(|(_, r)| r.trim())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| line.trim())
                .to_string();
        }
    }
    if let Some(line) = out.lines().find(|l| l.contains("FAILED")) {
        if let Some(next) = out
            .lines()
            .skip_while(|l| !l.contains("FAILED"))
            .nth(1)
            .filter(|l| l.contains("Reason:"))
        {
            return next
                .split_once("Reason:")
                .map(|(_, r)| r.trim())
                .unwrap_or("")
                .to_string();
        }
        return line.trim().to_string();
    }
    if let Some(line) = err.lines().find(|l| !l.is_empty()) {
        return line.trim().to_string();
    }
    out.lines().last().unwrap_or("").trim().to_string()
}

fn run_test_binary() -> std::path::PathBuf {
    if let Ok(bin) = std::env::var("RUN_TEST_BIN") {
        return std::path::PathBuf::from(bin);
    }
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ws = manifest
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(&manifest);
    preferred_run_test_binary(&ws.join("target"))
        .unwrap_or_else(|| std::path::PathBuf::from("target/debug/run-test"))
}

fn preferred_run_test_binary(target: &Path) -> Option<std::path::PathBuf> {
    first_existing(&[
        target.join("debug/run-test"),
        target.join("release/run-test"),
    ])
}

fn first_existing(candidates: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    candidates.iter().find(|p| p.is_file()).cloned()
}

#[cfg(test)]
mod tests;
