const __quenchApplyFinalModule01 = (name, originalRequire) => {
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
    const validateCpOptions = (options) => {
      if (
        options !== undefined &&
        (options === null || typeof options !== "object")
      ) {
        const error = new TypeError(
          "The options argument must be of type object",
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (options?.mode !== undefined) {
        const mode = options.mode;
        if (typeof mode !== "number") {
          const error = new TypeError(
            `The "mode" argument must be of type number. Received type ${typeof mode} ('${
              String(
                mode,
              )
            }')`,
          );
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        if (!Number.isInteger(mode) || mode < 0 || mode > 7) {
          const error = new RangeError(
            `The value of "mode" is out of range. It must be ${
              Number.isInteger(mode) ? ">= 0 && <= 7" : "an integer"
            }. Received ${String(mode)}`,
          );
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
      }
      if (options?.dereference === true && options?.verbatimSymlinks === true) {
        const error = new TypeError(
          "The 'dereference' and 'verbatimSymlinks' options cannot be used together",
        );
        error.code = "ERR_INCOMPATIBLE_OPTION_PAIR";
        throw error;
      }
    };
    const copyPath = (source, destination, options = {}) => {
      const copyValue = (value) => {
        if (!(value instanceof URL) && value?.protocol !== "file:") {
          return String(value);
        }
        try {
          return decodeURIComponent(value.pathname);
        } catch (_) {
          return value.pathname;
        }
      };
      const sourcePath = copyValue(source);
      const destinationPath = copyValue(destination);
      const normalizedSource = sourcePath
        .replace(/\\/g, "/")
        .replace(/\/+$/, "");
      const normalizedDestination = destinationPath
        .replace(/\\/g, "/")
        .replace(/\/+$/, "");
      if (typeof options.filter === "function") {
        const decision = options.filter(sourcePath, destinationPath);
        if (decision && typeof decision.then === "function") {
          const error = new TypeError(
            "The filter function must return a boolean synchronously",
          );
          error.code = "ERR_INVALID_RETURN_VALUE";
          throw error;
        }
        if (!decision) return;
      }
      let sourceStat = result.lstatSync(sourcePath);
      if (options.dereference && sourceStat.isSymbolicLink?.()) {
        sourceStat = result.statSync(sourcePath);
      }
      if (sourceStat.isSymbolicLink?.() && !options.dereference) {
        result.mkdirSync(
          result.dirname?.(destinationPath) ||
            destinationPath.replace(/\/[^/]*$/, "") ||
            ".",
          { recursive: true },
        );
        try {
          if (result.lstatSync(destinationPath).isSymbolicLink?.()) {
            result.unlinkSync(destinationPath);
          } else {
            if (options.force === false) return;
            const error = new Error(
              `EEXIST: file already exists, symlink '${destinationPath}'`,
            );
            error.code = "EEXIST";
            throw error;
          }
        } catch (error) {
          if (error.code === "EEXIST") throw error;
        }
        result.symlinkSync(result.readlinkSync(sourcePath), destinationPath);
        return;
      }
      if (sourceStat.isDirectory?.()) {
        if (
          normalizedDestination === normalizedSource ||
          normalizedDestination.startsWith(`${normalizedSource}/`)
        ) {
          const error = new Error(
            `Cannot copy ${sourcePath} to a subdirectory of itself, ${destinationPath}`,
          );
          error.code = "ERR_FS_CP_EINVAL";
          throw error;
        }
        if (options.errorOnExist) {
          try {
            result.lstatSync(destinationPath);
            const error = new Error(
              `Target already exists: ${destinationPath}`,
            );
            error.code = "ERR_FS_CP_EEXIST";
            throw error;
          } catch (error) {
            if (error.code === "ERR_FS_CP_EEXIST") throw error;
          }
        }
        try {
          const destinationStat = result.lstatSync(destinationPath);
          if (!destinationStat.isDirectory?.()) {
            const error = new Error(
              `Cannot copy a non-directory ${destinationPath} with directory ${sourcePath}`,
            );
            error.code = "ERR_FS_CP_DIR_TO_NON_DIR";
            throw error;
          }
        } catch (error) {
          if (error.code === "ERR_FS_CP_DIR_TO_NON_DIR") throw error;
        }
        if (!options.recursive) {
          const error = new TypeError(
            "Cannot copy a directory without recursive option",
          );
          error.code = "ERR_FS_EISDIR";
          throw error;
        }
        result.mkdirSync(destinationPath, { recursive: true });
        for (
          const entry of result.readdirSync(sourcePath, {
            withFileTypes: true,
          })
        ) {
          const childSource = `${sourcePath}/${entry.name}`;
          const childDestination = `${destinationPath}/${entry.name}`;
          if (
            typeof options.filter === "function" &&
            (() => {
              const decision = options.filter(childSource, childDestination);
              if (decision && typeof decision.then === "function") {
                const error = new TypeError(
                  "The filter function must return a boolean synchronously",
                );
                error.code = "ERR_INVALID_RETURN_VALUE";
                throw error;
              }
              return !decision;
            })()
          ) {
            continue;
          }
          copyPath(childSource, childDestination, options);
        }
        return;
      }
      try {
        const destinationStat = result.lstatSync(destinationPath);
        if (options.force === false) return;
        if (destinationStat.isDirectory?.()) {
          const error = new Error(
            `Cannot overwrite directory with non-directory: ${destinationPath}`,
          );
          error.code = "ERR_FS_CP_NON_DIR_TO_DIR";
          throw error;
        }
      } catch (error) {
        if (error.code === "ERR_FS_CP_NON_DIR_TO_DIR") throw error;
      }
      result.mkdirSync(destinationPath.replace(/\/[^/]*$/, "") || ".", {
        recursive: true,
      });
      result.copyFileSync(sourcePath, destinationPath);
      const setTimes = result.utimesSync || globalThis.__nodeFs?.utimesSync;
      if (options.preserveTimestamps && setTimes) {
        setTimes(destinationPath, sourceStat.atime, sourceStat.mtime);
      }
    };
    const copyPathAsync = async (sourcePath, destinationPath, options) => {
      if (typeof options.filter === "function") {
        const allowed = await options.filter(sourcePath, destinationPath);
        if (!allowed) return;
      }
      let sourceStat = result.lstatSync(sourcePath);
      if (options.dereference && sourceStat.isSymbolicLink?.()) {
        sourceStat = result.statSync(sourcePath);
      }
      if (typeof options.filter === "function") {
        const allowed = await options.filter(sourcePath, destinationPath);
        if (!allowed) return;
      }
      if (sourceStat.isDirectory?.()) {
        const normalizedSource = sourcePath
          .replace(/\\/g, "/")
          .replace(/\/+$/, "");
        const normalizedDestination = destinationPath
          .replace(/\\/g, "/")
          .replace(/\/+$/, "");
        if (
          normalizedDestination === normalizedSource ||
          normalizedDestination.startsWith(`${normalizedSource}/`)
        ) {
          const error = new Error(
            `Cannot copy ${sourcePath} to a subdirectory of itself, ${destinationPath}`,
          );
          error.code = "ERR_FS_CP_EINVAL";
          throw error;
        }
        if (options.errorOnExist) {
          try {
            result.lstatSync(destinationPath);
            const error = new Error(
              `Target already exists: ${destinationPath}`,
            );
            error.code = "ERR_FS_CP_EEXIST";
            throw error;
          } catch (error) {
            if (error.code === "ERR_FS_CP_EEXIST") throw error;
          }
        }
        try {
          const destinationStat = result.lstatSync(destinationPath);
          if (!destinationStat.isDirectory?.()) {
            const error = new Error(
              `Cannot copy a non-directory ${destinationPath} with directory ${sourcePath}`,
            );
            error.code = "ERR_FS_CP_DIR_TO_NON_DIR";
            throw error;
          }
        } catch (error) {
          if (error.code === "ERR_FS_CP_DIR_TO_NON_DIR") throw error;
        }
        if (!options.recursive) {
          const error = new TypeError(
            "Cannot copy a directory without recursive option",
          );
          error.code = "ERR_FS_EISDIR";
          throw error;
        }
        result.mkdirSync(destinationPath, { recursive: true });
        for (
          const entry of result.readdirSync(sourcePath, {
            withFileTypes: true,
          })
        ) {
          await copyPathAsync(
            `${sourcePath}/${entry.name}`,
            `${destinationPath}/${entry.name}`,
            options,
          );
        }
        return;
      }
      result.mkdirSync(destinationPath.replace(/\/[^/]*$/, "") || ".", {
        recursive: true,
      });
      result.copyFileSync(sourcePath, destinationPath);
    };
    {
      result.cp = (source, destination, options, callback) => {
        if (typeof options === "function") {
          callback = options;
          options = undefined;
        }
        validateCpOptions(options);
        if (typeof callback !== "function") {
          const error = new TypeError(
            "The callback argument must be of type function",
          );
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        queueMicrotask(() => {
          Promise.resolve()
            .then(() =>
              typeof options.filter === "function"
                ? copyPathAsync(source, destination, options)
                : copyPath(source, destination, options)
            )
            .then(() => callback(null), callback);
        });
      };
    }
    {
      result.cpSync = (source, destination, options) => {
        validateCpOptions(options);
        copyPath(source, destination, options);
      };
    }
    result.promises ||= {};
    {
      result.promises.cp = async (source, destination, options) => {
        validateCpOptions(options);
        copyPath(source, destination, options);
      };
    }
    globalThis.__quenchFsCopy = result.promises.cp;
  }
  if (normalized === "fs/promises") {
    if (!globalThis.__quenchFsCopy) {
      __quenchApplyFinalModule01("fs", originalRequire);
    }
    result.cp ||= globalThis.__quenchFsCopy;
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
