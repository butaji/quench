//! Data for `process`'s lazily defined surface.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use super::event_constructor_shared_vm::get;

pub(crate) fn process_constructor(context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(context.undefined())
}

pub(crate) fn throw_proto_access(context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let error = context.error_rooted("Accessing Object.prototype.__proto__ has been disallowed with --disable-proto=throw")?;
    let code = context.string_rooted("ERR_PROTO_ACCESS");
    context.assign_named_property_rooted(error, "code", code)?;
    Err(context.throw(error))
}

pub(crate) fn initialize(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let global = context.global_root()?;
    let process = get(context, global, "process")?;
    let mode = context.host_mut().shared_state().borrow().cli_options.string_value("--disable-proto").map(str::to_owned);
    if let Some(mode) = mode {
        let object = get(context, global, "Object")?;
        let prototype = get(context, object, "prototype")?;
        let key = context.string_rooted("__proto__");
        match mode.as_str() {
            "delete" => context.delete_property_strict_rooted(prototype, key)?,
            "throw" => {
                let throw = context.host_function_named(crate::host::shared_vm::operation("processThrowProtoAccess"), "")?;
                let descriptor = context.null_object_rooted()?;
                context.assign_named_property_rooted(descriptor, "get", throw)?;
                context.assign_named_property_rooted(descriptor, "set", throw)?;
                let yes = context.boolean(true);
                let no = context.boolean(false);
                context.assign_named_property_rooted(descriptor, "configurable", yes)?;
                context.assign_named_property_rooted(descriptor, "enumerable", no)?;
                context.define_property_descriptor_rooted(prototype, key, descriptor)?;
            }
            _ => {}
        }
    }
    let emitter = get(context, global, "__nodeEventEmitter")?;
    let emitter_prototype = get(context, emitter, "prototype")?;
    let constructor = context.host_constructor_named(crate::host::shared_vm::operation("processSurfaceConstructor"), "process")?;
    let prototype = get(context, constructor, "prototype")?;
    context.set_prototype_rooted(prototype, emitter_prototype)?;
    context.set_prototype_rooted(process, prototype)?;
    Ok(())
}

/// Process environment and invocation values belong to the running process,
/// after the snapshot/source-capture boundary and before user preloads.
pub(crate) fn configure(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let global = context.global_root()?;
    let process = get(context, global, "process")?;
    // Avoid observing the lazy process.env view when coverage was not requested.
    if super::process_shared_vm::environment_names(context, "NODE_V8_COVERAGE") {
        let features = get(context, process, "features")?;
        let inspector = get(context, features, "inspector")?;
        let env = get(context, process, "env")?;
        let coverage = get(context, env, "NODE_V8_COVERAGE")?;
        if context.truthy_rooted(coverage)? && !context.truthy_rooted(inspector)? {
            let warning = get(context, process, "emitWarning")?;
            let message = context.string_rooted("The inspector is disabled, coverage could not be collected");
            let kind = context.string_rooted("Warning");
            context.call_rooted(warning, process, &[message, kind])?;
            let empty = context.string_rooted("");
            context.assign_named_property_rooted(env, "NODE_V8_COVERAGE", empty)?;
        }
    }
    let requested = get(context, global, "__quench_cli_title")?;
    let title = if context.truthy_rooted(requested)? { requested } else {
        let current = get(context, process, "title")?;
        if context.truthy_rooted(current)? { current } else { context.string_rooted("node") }
    };
    context.assign_named_property_rooted(process, "title", title)?;
    let key = context.string_rooted("__quench_cli_title");
    context.delete_property_strict_rooted(global, key)?;
    // Identity, tag, features, invocation arguments, kill and loadEnvFile are installed
    // by process_shared_vm before this boundary; they have one Rust owner.
    Ok(())
}

/// Native capability families are admitted independently. Observing one family
/// must not load an unrelated SDK policy.
pub(crate) const LAZY_GROUPS: &[(&[&str], &str)] = &[
    (&["ref", "unref", "getBuiltinModule",
        #[cfg(unix)] "availableMemory", #[cfg(unix)] "constrainedMemory"], "processCoreInitialize"),
    (SURFACE_MEMBERS, "processSurfaceInitialize"),
];

const SURFACE_MEMBERS: &[&str] = &[
    "getActiveResourcesInfo",
    "_getActiveRequests",
    #[cfg(not(unix))] "availableMemory",
    #[cfg(not(unix))] "constrainedMemory",
    "setSourceMapsEnabled",
    "abort",
    "sourceMapsEnabled",
    "debugPort",
    "allowedNodeEnvironmentFlags",
    "finalization",
    "permission",
    "resourceUsage",
    "memoryUsage",
    "cpuUsage",
    "threadCpuUsage",
    // Node's own wrapper over the native system call (`process_metrics_shared_vm`).
    "execve",
];

pub(crate) fn initialize_core(c: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let process = c.host_mut().shared_state().borrow().process_module
        .ok_or_else(|| RootedError::host("native process capabilities have no owner"))?;
    #[cfg(unix)]
    super::process_memory_shared_vm::install(c)?;
    c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        let symbol = get(c, global, "Symbol")?;
        let intern = get(c, symbol, "for")?;
        for (name, operation, key) in [
            ("ref", "processRef", "nodejs.ref"), ("unref", "processUnref", "nodejs.unref"),
        ] {
            let key = c.string_rooted(key);
            let key = c.call_rooted(intern, symbol, &[key])?;
            let method = c.host_constructor_named_with_data(crate::host::shared_vm::operation(operation), name, key)?;
            c.assign_named_property_rooted(process, name, method)?;
        }
        Ok(())
    })?;
    let method = c.host_constructor_named(crate::host::shared_vm::operation("processGetBuiltinModule"), "getBuiltinModule")?;
    c.assign_named_property_rooted(process, "getBuiltinModule", method)?;
    Ok(c.undefined())
}

/// Node's reference protocol: the symbol has priority when truthy, including
/// when it holds a non-callable value. Public `.call` hooks are never consulted.
pub(crate) fn reference<H: quench_runtime::Host, const REF: bool>(
    c: &mut NativeContext<'_, H>, _: RootId, args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let target = args.first().copied().unwrap_or(undefined);
    if c.rooted_value(target).is_some_and(|value| value.is_null() || value.is_undefined()) {
        return Ok(undefined);
    }
    let key = c.host_function_data()?;
    let mut method = c.get_property_rooted(target, key)?;
    if !c.truthy_rooted(method)? {
        method = c.get_named_property_rooted(target, if REF { "ref" } else { "unref" })?;
    }
    if c.is_callable_rooted(method)? { c.call_rooted(method, target, &[])?; }
    Ok(undefined)
}

#[cfg(test)]
mod native_core_tests {
    #[test]
    fn reference_protocol_and_builtin_lookup_keep_node_boundaries() {
        let source = r#"
          for (const name of ['ref', 'unref']) {
            const fn = process[name], key = Symbol.for('nodejs.' + name);
            if (fn.name !== name || fn.length !== 1 || !Object.hasOwn(fn, 'prototype')) throw Error('metadata');
            for (const value of [null, undefined, 0, false, '', 'x'])
              if (fn(value) !== undefined) throw Error('primitive completion');
            for (const symbolValue of [0, false, null, undefined, true, {}, 'truthy']) {
              const trace = [];
              const target = {
                get [key]() {trace.push('symbol');return symbolValue;},
                get [name]() {trace.push('fallback');return function() {
                  if (this !== target || arguments.length !== 0) throw Error('call'); trace.push('invoke');
                };},
              };
              if (fn.call({}, target, 'ignored') !== undefined) throw Error('return');
              const expected = symbolValue ? ['symbol'] : ['symbol', 'fallback', 'invoke'];
              if (JSON.stringify(trace) !== JSON.stringify(expected)) throw Error('lookup order');
            }
            const error = {};
            for (const target of [{get [key]() {throw error;}}, {[key]() {throw error;}},
              {[key]: false, get [name]() {throw error;}}]) {
              try {fn(target);throw Error('missing throw');} catch (caught) {if(caught!==error)throw caught;}
            }
            const target = {[key]: function() {if(this!==target)throw Error('receiver');}};
            target[key].call = function() {throw Error('mutable call hook');};
            fn(target);
            if (Object.getPrototypeOf(new fn()) !== fn.prototype) throw Error('constructor');
          }
          const builtin = process.getBuiltinModule;
          if (builtin.name !== 'getBuiltinModule' || builtin.length !== 1 || !Object.hasOwn(builtin,'prototype')) throw Error('builtin metadata');
          if (builtin('path') !== require('path') || builtin('node:path') !== require('path') ||
              builtin('process') !== process || builtin('not-a-builtin') !== undefined) throw Error('canonical loader');
          const path = builtin('path'), replacement = {};
          require.cache.path = {exports: replacement};
          if (require('path') !== replacement || builtin('path') !== path) throw Error('builtin cache boundary');
          delete require.cache.path;
          for (const id of ['internal/async_hooks', 'node:internal/async_hooks',
            'internal/timers', 'node:internal/timers', 'vfs', 'node:vfs']) {
            if (builtin(id) !== undefined) throw Error('builtin visibility');
          }
          for(const value of [null,undefined,1,false]) {
            try {builtin(value);throw Error('missing type error');} catch(e) {
              if(e.name!=='TypeError'||e.code!=='ERR_INVALID_ARG_TYPE')throw e;
            }
          }
        "#;
        let program = quench_runtime::Engine::specialize(source, "native-process-core.js").unwrap();
        let host = crate::NodeHost::new(vec!["quench-node".into()]);
        quench_runtime::Runtime::new(host).execute(&program).unwrap();
    }
}

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
