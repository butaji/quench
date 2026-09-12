//! Polyfill: `module-surface-final-01-tail-02`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchApplyFinalModule01 = (name, originalRequire) => {
  const normalized = String(name).replace(/^node:/, "");
  if (
    normalized === "diagnostics_channel" &&
    globalThis.__nodeDiagnosticsChannel
  ) {
    return globalThis.__nodeDiagnosticsChannel;
  }
  const internalFallback = __quenchInternalStreamFallback(normalized);
  if (internalFallback) return internalFallback;
  if (
    normalized === "internal/vfs/stats" && globalThis.__quenchVfsStatsHelpers
  ) {
    return globalThis.__quenchVfsStatsHelpers;
  }
  if (normalized === "internal/vfs/fd") {
    return {
      getVirtualFd(fd) {
        return globalThis.__quenchVfsFdHandles?.get(fd);
      },
    };
  }
  if (normalized === "internal/vfs/router") {
    const path = globalThis.__nodePath;
    return {
      isUnderMountPoint(value, mountPoint) {
        const valuePath = path.resolve(value);
        const mountPath = path.resolve(mountPoint);
        return (
          mountPath === path.parse(mountPath).root ||
          valuePath === mountPath ||
          valuePath.startsWith(`${mountPath}${path.sep}`)
        );
      },
      getRelativePath(value, mountPoint) {
        const relative = path.relative(
          path.resolve(mountPoint),
          path.resolve(value),
        );
        return relative ? `/${relative.split(path.sep).join("/")}` : "/";
      },
      isAbsolutePath: path.isAbsolute,
    };
  }
  if (normalized === "sqlite") {
    return {
      DatabaseSync: function DatabaseSync() {},
      StatementSync: function StatementSync() {},
      constants: {},
    };
  }
  if (normalized === "inspector") {
    return {
      open: () => undefined,
      close: () => undefined,
      url: () => undefined,
      waitForDebugger: () => undefined,
      Session: function Session() {},
      console: {},
    };
  }
  let result = originalRequire(name);
  if (normalized === "timers") {
    result.promises = originalRequire("timers/promises");
    const promisifyCustom = Symbol.for("nodejs.util.promisify.custom");
    result.setTimeout[promisifyCustom] = result.promises.setTimeout;
    result.setImmediate[promisifyCustom] = result.promises.setImmediate;
  }
  result = __quenchApplyFinalSurface(normalized, result);
  if (normalized === "buffer" && globalThis.Blob) {
    result.Blob = globalThis.Blob;
  }
  if (normalized === "fs") {
    globalThis.__quenchFsConstantsModule ||= result.constants;
    result.constants = globalThis.__quenchFsConstantsModule;
    if (result.promises) result.promises.constants = result.constants;
    if (result.promises) {
      globalThis.__quenchFsPromisesModule ||= result.promises;
      result.promises = globalThis.__quenchFsPromisesModule;
    }
    // `cp`, `cpSync`, and `promises.cp` are Rust capabilities installed by
    // the fs module. Keep this surface pass-through so one implementation
    // owns validation, errors, symlink policy, and callback timing.
  }
  if (normalized === "fs/promises") {
    // The promises namespace already carries the Rust `cp` capability.
  }
  if (normalized === "test") {
    return __quenchTestModuleFallbacks(result, originalRequire, name);
  }
  if (normalized === "util") {
    result.types ||= Object.create(null);
    __quenchUtilTypesFallbacks(result.types);
    return result;
  }
  if (normalized === "util/types") {
    const util = originalRequire("util");
    util.types ||= result;
    return __quenchUtilTypesFallbacks(util.types);
  }
  return result;
};
globalThis.__quenchFinalizeModule = (name, originalRequire, result) =>
  __quenchApplyFinalModule01(name, originalRequire, result);
if (globalThis.require) {
  const originalRequire = globalThis.require;
  const wrappedRequire = (name) =>
    __quenchApplyFinalModule01(name, originalRequire);
  Object.assign(wrappedRequire, originalRequire);
  wrappedRequire.cache = originalRequire.cache || Object.create(null);
  wrappedRequire.extensions = originalRequire.extensions || Object.create(null);
  wrappedRequire.resolve = globalThis.__quenchRequireResolve ||
    originalRequire.resolve;
  if (globalThis.__quenchRequireResolvePaths) {
    wrappedRequire.resolve.paths = globalThis.__quenchRequireResolvePaths;
  }
  const moduleApi = originalRequire("module");
  moduleApi._extensions = wrappedRequire.extensions;
  globalThis.require = wrappedRequire;
}
"#);
