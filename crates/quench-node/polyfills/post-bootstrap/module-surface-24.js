{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      const normalized = String(name).replace(/^node:/, "");
      if (normalized === "wasi") {
        return {
          WASI: function WASI() {},
          getImportObject: () => ({}),
          WASI_VERSION: " wasi_snapshot_preview1",
          WASI_PREVIEW1: " wasi_snapshot_preview1"
        };
      }
      return originalRequire(name);
    };
  }
}
