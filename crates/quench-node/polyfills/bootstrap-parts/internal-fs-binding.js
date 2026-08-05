globalThis.__quenchInternalFsBinding = {
  openFileHandle: (_path, _flags, _mode, _req, _context) => undefined,
  readdir: (path) => {
    const names = globalThis.__nodeFs.readdirSync(path);
    return [names, names.map(() => 1)];
  }
};
globalThis.__quenchInternalFallbackBinding = { fstat: () => undefined };
globalThis.__quenchInternalBindingCore = (binding) => {
  if (binding === "fs") return globalThis.__quenchInternalFsBinding;
  if (binding === "os") return globalThis.__quenchInternalOsBinding;
  if (binding === "debug")
    return { getGenericUsageCount: () => __nodeAllocatorCounts.zeroFilled };
};
