//! Data for `process`'s lazily defined surface.

/// The `process` members defined on first access (`processSurfaceInitialize`).
pub(crate) const LAZY_MEMBERS: &[&str] = &[
    "getActiveResourcesInfo",
    "_getActiveRequests",
    "availableMemory",
    "constrainedMemory",
    "setSourceMapsEnabled",
    "abort",
    "sourceMapsEnabled",
    "debugPort",
    "allowedNodeEnvironmentFlags",
    "getBuiltinModule",
    "finalization",
    "permission",
    "resourceUsage",
    "memoryUsage",
    "cpuUsage",
    "threadCpuUsage",
    // Node's own wrapper over the native system call (`process_metrics_shared_vm`).
    "execve",
    "ref",
    "unref",
];

/// Node's own CLI options (the source list of `process.allowedNodeEnvironmentFlags`).
pub(crate) const NODE_FLAGS: &[&str] = &[
    "--abort-on-uncaught-exception",
    "--addons",
    "--allow-addons",
    "--allow-child-process",
    "--allow-fs-read",
    "--allow-fs-vfs",
    "--allow-fs-write",
    "--allow-inspector",
    "--allow-net",
    "--allow-openssl-store",
    "--allow-wasi",
    "--allow-worker",
    "--async-context-frame",
    "--bench-isolation",
    "--bench-name-pattern",
    "--bench-reporter",
    "--bench-reporter-destination",
    "--bench-samples",
    "--bench-warmup",
    "--conditions",
    "--debug-arraybuffer-allocations",
    "--deprecation",
    "--diagnostic-dir",
    "--disable-proto",
    "--disable-sigusr1",
    "--disable-warning",
    "--disable-wasm-trap-handler",
    "--disallow-code-generation-from-strings",
    "--dns-result-order",
    "--enable-etw-stack-walking",
    "--enable-fips",
    "--enable-fips-indicator-events",
    "--enable-network-family-autoselection",
    "--enable-source-maps",
    "--entry-url",
    "--es-module-specifier-resolution",
    "--experimental-abortcontroller",
    "--experimental-addon-modules",
    "--experimental-bench",
    "--experimental-detect-module",
    "--experimental-dtls",
    "--experimental-eventsource",
    "--experimental-fetch",
    "--experimental-global-customevent",
    "--experimental-global-navigator",
    "--experimental-global-webcrypto",
    "--experimental-import-meta-resolve",
    "--experimental-import-text",
    "--experimental-json-modules",
    "--experimental-loader",
    "--experimental-modules",
    "--experimental-package-map",
    "--experimental-print-required-tla",
    "--experimental-quic",
    "--experimental-repl-await",
    "--experimental-report",
    "--experimental-require-module",
    "--experimental-shadow-realm",
    "--experimental-specifier-resolution",
    "--experimental-sqlite",
    "--experimental-stream-iter",
    "--experimental-strip-types",
    "--experimental-test-isolation",
    "--experimental-top-level-await",
    "--experimental-vfs",
    "--experimental-vm-modules",
    "--experimental-wasi-unstable-preview1",
    "--experimental-wasm-modules",
    "--experimental-web-worker",
    "--experimental-websocket",
    "--experimental-webstorage",
    "--experimental-worker",
    "--expose-gc",
    "--extra-info-on-fatal-exception",
    "--force-async-hooks-checks",
    "--force-context-aware",
    "--force-fips",
    "--force-node-api-uncaught-exceptions-policy",
    "--frozen-intrinsics",
    "--global-search-paths",
    "--heapsnapshot-near-heap-limit",
    "--heapsnapshot-signal",
    "--http-parser",
    "--icu-data-dir",
    "--import",
    "--input-type",
    "--insecure-http-parser",
    "--interpreted-frames-native-stack",
    "--jitless",
    "--loader",
    "--localstorage-file",
    "--max-heap-size",
    "--max-http-header-size",
    "--max-old-space-size",
    "--max-old-space-size-percentage",
    "--max-semi-space-size",
    "--napi-modules",
    "--network-family-autoselection",
    "--network-family-autoselection-attempt-timeout",
    "--no-addons",
    "--no-allow-addons",
    "--no-allow-child-process",
    "--no-allow-inspector",
    "--no-allow-wasi",
    "--no-allow-worker",
    "--no-async-context-frame",
    "--no-debug-arraybuffer-allocations",
    "--no-deprecation",
    "--no-disable-sigusr1",
    "--no-disable-wasm-trap-handler",
    "--no-enable-fips",
    "--no-enable-source-maps",
    "--no-entry-url",
    "--no-experimental-addon-modules",
    "--no-experimental-detect-module",
    "--no-experimental-eventsource",
    "--no-experimental-global-navigator",
    "--no-experimental-import-meta-resolve",
    "--no-experimental-import-text",
    "--no-experimental-print-required-tla",
    "--no-experimental-repl-await",
    "--no-experimental-require-module",
    "--no-experimental-shadow-realm",
    "--no-experimental-sqlite",
    "--no-experimental-vm-modules",
    "--no-experimental-websocket",
    "--no-experimental-webstorage",
    "--no-extra-info-on-fatal-exception",
    "--no-force-async-hooks-checks",
    "--no-force-context-aware",
    "--no-force-fips",
    "--no-force-node-api-uncaught-exceptions-policy",
    "--no-frozen-intrinsics",
    "--no-global-search-paths",
    "--no-insecure-http-parser",
    "--no-network-family-autoselection",
    "--no-node-snapshot",
    "--no-openssl-legacy-provider",
    "--no-openssl-shared-config",
    "--no-pending-deprecation",
    "--no-permission",
    "--no-preserve-symlinks",
    "--no-preserve-symlinks-main",
    "--no-report-compact",
    "--no-report-exclude-env",
    "--no-report-exclude-network",
    "--no-report-on-fatalerror",
    "--no-report-on-signal",
    "--no-report-uncaught-exception",
    "--no-require-module",
    "--no-strip-types",
    "--no-test-force-exit",
    "--no-test-only",
    "--no-test-randomize",
    "--no-throw-deprecation",
    "--no-tls-max-v1.2",
    "--no-tls-max-v1.3",
    "--no-tls-min-v1.0",
    "--no-tls-min-v1.1",
    "--no-tls-min-v1.2",
    "--no-tls-min-v1.3",
    "--no-trace-deprecation",
    "--no-trace-env",
    "--no-trace-env-js-stack",
    "--no-trace-env-native-stack",
    "--no-trace-exit",
    "--no-trace-promises",
    "--no-trace-sigint",
    "--no-trace-sync-io",
    "--no-trace-tls",
    "--no-trace-uncaught",
    "--no-trace-warnings",
    "--no-track-heap-objects",
    "--no-use-bundled-ca",
    "--no-use-env-proxy",
    "--no-use-openssl-ca",
    "--no-use-system-ca",
    "--no-verify-base-objects",
    "--no-warnings",
    "--no-watch",
    "--no-watch-preserve-output",
    "--no-zero-fill-buffers",
    "--node-memory-debug",
    "--node-snapshot",
    "--openssl-config",
    "--openssl-legacy-provider",
    "--openssl-shared-config",
    "--pending-deprecation",
    "--perf-basic-prof",
    "--perf-basic-prof-only-functions",
    "--perf-prof",
    "--perf-prof-unwinding-info",
    "--permission",
    "--permission-audit",
    "--preserve-symlinks",
    "--preserve-symlinks-main",
    "--prof-process",
    "--redirect-warnings",
    "--report-compact",
    "--report-dir",
    "--report-directory",
    "--report-exclude-env",
    "--report-exclude-network",
    "--report-filename",
    "--report-on-fatalerror",
    "--report-on-signal",
    "--report-signal",
    "--report-uncaught-exception",
    "--require",
    "--require-module",
    "--secure-heap",
    "--secure-heap-min",
    "--snapshot-blob",
    "--stack-trace-limit",
    "--strip-types",
    "--test-coverage-branches",
    "--test-coverage-exclude",
    "--test-coverage-functions",
    "--test-coverage-include",
    "--test-coverage-include-all",
    "--test-coverage-lines",
    "--test-force-exit",
    "--test-global-setup",
    "--test-isolation",
    "--test-name-pattern",
    "--test-only",
    "--test-random-seed",
    "--test-randomize",
    "--test-reporter",
    "--test-reporter-destination",
    "--test-rerun-failures",
    "--test-shard",
    "--test-skip-pattern",
    "--throw-deprecation",
    "--title",
    "--tls-cipher-list",
    "--tls-keylog",
    "--tls-max-v1.2",
    "--tls-max-v1.3",
    "--tls-min-v1.0",
    "--tls-min-v1.1",
    "--tls-min-v1.2",
    "--tls-min-v1.3",
    "--trace-deprecation",
    "--trace-env",
    "--trace-env-js-stack",
    "--trace-env-native-stack",
    "--trace-event-categories",
    "--trace-event-file-pattern",
    "--trace-events-enabled",
    "--trace-exit",
    "--trace-promises",
    "--trace-require-module",
    "--trace-sigint",
    "--trace-sync-io",
    "--trace-tls",
    "--trace-uncaught",
    "--trace-warnings",
    "--track-heap-objects",
    "--unhandled-rejections",
    "--use-bundled-ca",
    "--use-env-proxy",
    "--use-largepages",
    "--use-openssl-ca",
    "--use-system-ca",
    "--v8-pool-size",
    "--verify-base-objects",
    "--warnings",
    "--watch",
    "--watch-kill-signal",
    "--watch-path",
    "--watch-preserve-output",
    "--webstorage",
    "--worker-snapshot",
    "--zero-fill-buffers",
    "-C",
    "-r",
];

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = c.string_rooted(name);
    c.get_property_rooted(object, key)
}

fn nullish(c: &mut Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_none_or(|value| value.is_undefined() || value.is_null())
}

/// The message and code of `--disable-proto=throw`'s accessor.
const PROTO_ACCESS_MESSAGE: &str = "Accessing Object.prototype.__proto__ is disabled";
const PROTO_ACCESS_CODE: &str = "ERR_PROTO_ACCESS";

/// What `--disable-proto[=]mode` asks of `Object.prototype.__proto__`.
fn disable_proto_mode(exec_argv: &[String]) -> Option<&str> {
    exec_argv.iter().enumerate().find_map(|(index, argument)| match argument.strip_prefix("--disable-proto") {
        Some(rest) if rest.starts_with('=') => Some(&rest[1..]),
        Some("") => exec_argv.get(index + 1).map(String::as_str),
        _ => None,
    })
}

/// Completes `process` after bootstrap: `--disable-proto`, its prototype chain through
/// `EventEmitter.prototype`, `Symbol.toStringTag`, and the defaults Node supplies.
pub(crate) fn initialize(c: &mut Context<'_>) -> Result<(), RootedError> {
    let global = c.global_root()?;
    let process = get(c, global, "process")?;
    if nullish(c, process) {
        return Ok(());
    }
    let exec_argv = c.host_mut().shared_state().borrow().exec_argv.clone();
    let object = get(c, global, "Object")?;
    let object_prototype = get(c, object, "prototype")?;
    let proto_key = c.string_rooted("__proto__");
    match disable_proto_mode(&exec_argv) {
        Some("delete") => {
            c.delete_property_rooted(object_prototype, proto_key)?;
        }
        Some("throw") => {
            let accessor = c.host_function(crate::host::shared_vm::operation("processProtoAccess"))?;
            let descriptor = c.null_object_rooted()?;
            for name in ["get", "set"] {
                c.assign_named_property_rooted(descriptor, name, accessor)?;
            }
            let yes = c.boolean(true);
            let no = c.boolean(false);
            c.assign_named_property_rooted(descriptor, "configurable", yes)?;
            c.assign_named_property_rooted(descriptor, "enumerable", no)?;
            c.define_property_descriptor_rooted(object_prototype, proto_key, descriptor)?;
        }
        _ => {}
    }
    let symbol = get(c, global, "Symbol")?;
    let tag_key = get(c, symbol, "toStringTag")?;
    let tag = c.get_property_rooted(process, tag_key)?;
    if !c.truthy_rooted(tag)? {
        let name = c.string_rooted("process");
        c.set_property_rooted(process, tag_key, name, process)?;
    }
    let emitter = get(c, global, "__nodeEventEmitter")?;
    if c.is_object_rooted(emitter)? {
        let emitter_prototype = get(c, emitter, "prototype")?;
        // `process`'s constructor is a function named `process` whose prototype inherits
        // from `EventEmitter.prototype`.
        let constructor = c.host_constructor_named(crate::host::shared_vm::operation("processConstructor"), "process")?;
        let prototype = get(c, constructor, "prototype")?;
        c.set_prototype_rooted(prototype, emitter_prototype)?;
        c.set_prototype_rooted(process, prototype)?;
    }
    let kill = get(c, process, "kill")?;
    if !c.truthy_rooted(kill)? {
        let kill = c.host_function_named(crate::host::shared_vm::operation("processKillPortable"), "kill")?;
        c.assign_named_property_rooted(process, "kill", kill)?;
    }
    let argv = get(c, process, "execArgv")?;
    if nullish(c, argv) {
        let empty = c.array_rooted(&[])?;
        c.assign_named_property_rooted(process, "execArgv", empty)?;
    }
    let argv0 = get(c, process, "argv0")?;
    if !c.truthy_rooted(argv0)? {
        let node = c.string_rooted("node");
        c.assign_named_property_rooted(process, "argv0", node)?;
    }
    let mut features = get(c, process, "features")?;
    if !c.truthy_rooted(features)? {
        features = c.object_rooted()?;
        c.assign_named_property_rooted(process, "features", features)?;
    }
    let inspector = get(c, features, "inspector")?;
    if nullish(c, inspector) {
        let no = c.boolean(false);
        c.assign_named_property_rooted(features, "inspector", no)?;
    }
    // Node's start-up coverage hook warns when NODE_V8_COVERAGE is requested without the
    // inspector profiler, then clears it so further children do not repeat the request.
    let env = get(c, process, "env")?;
    if c.is_object_rooted(env)? {
        let coverage = get(c, env, "NODE_V8_COVERAGE")?;
        let inspector = get(c, features, "inspector")?;
        if c.truthy_rooted(coverage)? && !c.truthy_rooted(inspector)? {
            let emit = get(c, process, "emitWarning")?;
            let message = c.string_rooted("The inspector is disabled, coverage could not be collected");
            let kind = c.string_rooted("Warning");
            c.call_rooted(emit, process, &[message, kind])?;
            let empty = c.string_rooted("");
            c.assign_named_property_rooted(env, "NODE_V8_COVERAGE", empty)?;
        }
    }
    let cli_title_key = c.string_rooted("__quench_cli_title");
    let cli_title = c.get_property_rooted(global, cli_title_key)?;
    let title = if c.truthy_rooted(cli_title)? {
        cli_title
    } else {
        let current = get(c, process, "title")?;
        if c.truthy_rooted(current)? { current } else { c.string_rooted("node") }
    };
    c.assign_named_property_rooted(process, "title", title)?;
    c.delete_property_rooted(global, cli_title_key)?;
    let load_env_file = get(c, process, "loadEnvFile")?;
    if !c.truthy_rooted(load_env_file)? {
        let function = c.host_function_named(crate::host::shared_vm::operation("processLoadEnvFileNone"), "")?;
        c.assign_named_property_rooted(process, "loadEnvFile", function)?;
    }
    Ok(())
}

/// `process`'s constructor: Node's is not meant to be called; it does nothing.
pub(crate) fn constructor(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(c.undefined())
}

/// `--disable-proto=throw`'s `__proto__` accessor.
pub(crate) fn proto_access(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let error = c.error_rooted(PROTO_ACCESS_MESSAGE)?;
    let code = c.string_rooted(PROTO_ACCESS_CODE);
    c.assign_named_property_rooted(error, "code", code)?;
    Err(c.throw(error))
}

/// `process.loadEnvFile` where the host provides none.
pub(crate) fn load_env_file_none(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(c.undefined())
}

/// The signals `process.kill` names where the host has no signal table.
const PORTABLE_SIGNALS: &[(&str, f64)] = &[("SIGHUP", 1.0), ("SIGINT", 2.0), ("SIGQUIT", 3.0), ("SIGKILL", 9.0), ("SIGTERM", 15.0)];
/// The errno names `process._kill` reports.
const KILL_ERRORS: &[(f64, &str)] = &[(1.0, "EPERM"), (2.0, "ENOENT"), (3.0, "ESRCH"), (13.0, "EACCES"), (22.0, "EINVAL")];

/// `process.kill(pid[, signal])` over `process._kill`, where the host has no signal table.
pub(crate) fn kill_portable(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let pid = args.first().copied().unwrap_or(missing);
    let pid_number = match c.rooted_value(pid) {
        Some(value) if value.as_number().is_some() => value.as_number(),
        _ => c.string_text(pid)?.filter(|text| !text.is_empty()).map(|text| text.trim().parse::<f64>().unwrap_or(f64::NAN)),
    }
    .unwrap_or(f64::NAN);
    if pid_number.fract() != 0.0 || !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&pid_number) {
        return Err(super::util_shared_vm::argument_type_error_with_expected(c, pid, "pid", "of type number")?);
    }
    let signal = args.get(1).copied().unwrap_or(missing);
    let number = match c.rooted_value(signal).and_then(|value| value.as_number()).filter(|number| number.fract() == 0.0) {
        Some(number) => number,
        None => {
            let name = if c.truthy_rooted(signal)? { c.to_string(signal)? } else { "SIGTERM".to_owned() };
            match PORTABLE_SIGNALS.iter().find(|(signal, _)| *signal == name) {
                Some((_, number)) => *number,
                None => {
                    let error = c.type_error_rooted(&format!("Unknown signal: {name}"))?;
                    let code = c.string_rooted("ERR_UNKNOWN_SIGNAL");
                    c.assign_named_property_rooted(error, "code", code)?;
                    return Err(c.throw(error));
                }
            }
        }
    };
    let global = c.global_root()?;
    let process = get(c, global, "process")?;
    let raw_kill = get(c, process, "_kill")?;
    let number = c.number(number);
    let result = c.call_rooted(raw_kill, process, &[pid, number])?;
    if c.truthy_rooted(result)? {
        let errno = c.rooted_value(result).and_then(|value| value.as_number()).unwrap_or(f64::NAN);
        let name = KILL_ERRORS.iter().find(|(code, _)| *code == errno).map_or("UNKNOWN", |(_, name)| *name);
        let error = c.error_rooted(&format!("kill {name}"))?;
        let code = c.string_rooted(name);
        c.assign_named_property_rooted(error, "code", code)?;
        return Err(c.throw(error));
    }
    Ok(c.boolean(true))
}
