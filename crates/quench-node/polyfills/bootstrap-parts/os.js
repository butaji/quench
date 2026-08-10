const __quenchOriginalRequireWithOsHelpers = globalThis.require;
globalThis.__nodeOsInitialized = false;
const __quenchOsFallback = {
  homedir: () =>
    globalThis.__quench_homedir || globalThis.process?.env?.HOME || "/",
  tmpdir: () => {
    const env = globalThis.process?.env || {};
    return (
      String(
        env.TMPDIR ||
          env.TMP ||
          env.TEMP ||
          globalThis.__quench_tmpdir ||
          "/tmp",
      ).replace(/\/$/, "") || "/"
    );
  },
  userInfo: (options = {}) => {
    const value = {
      uid: 0,
      gid: 0,
      username: "unknown",
      homedir: __quenchOsFallback.homedir(),
      shell: "/bin/sh",
    };
    if (options.encoding === "buffer") {
      for (const key of ["username", "homedir", "shell"]) {
        value[key] = Buffer.from(value[key]);
      }
    }
    return value;
  },
};
globalThis.__quenchInternalOsBinding = {
  getHomeDirectory: (context) => {
    context.value = globalThis.__quench_homedir || "/";
  },
};
const __quenchOsHomeDirectory = () => {
  const context = {};
  globalThis.__quenchInternalOsBinding.getHomeDirectory(context);
  if (context.syscall) {
    throw new Error(
      `A system error occurred: ${context.syscall} returned ${context.code} (${context.message})`,
    );
  }
  return context.value;
};
globalThis.__quenchOsHomeDirectory = __quenchOsHomeDirectory;
__quenchOsFallback.homedir = __quenchOsHomeDirectory;
for (const [name, fallback] of Object.entries(__quenchOsFallback)) {
  if (globalThis.__nodeOs[name] === undefined) {
    globalThis.__nodeOs[name] = fallback;
  }
}
for (const name of ["homedir", "tmpdir"]) {
  if (typeof globalThis.__nodeOs[name] === "function") {
    globalThis.__nodeOs[name].toString = () => globalThis.__nodeOs[name]();
  }
}
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "os") {
    globalThis.__nodeOsInitialized = true;
    return globalThis.__nodeOs;
  }
  return __quenchOriginalRequireWithOsHelpers(specifier);
};
