const __quenchOriginalRequireWithV8 = globalThis.require;
const __quenchV8 = {
  getHeapStatistics: () => ({
    total_heap_size: 0,
    total_heap_size_executable: 0,
    total_physical_size: 0,
    total_available_size: 0,
    used_heap_size: 0,
    heap_size_limit: 0,
    malloced_memory: 0,
    external_memory: 0,
    peak_malloced_memory: 0,
    does_zap_garbage: 0
  }),
  getHeapCodeStatistics: () => ({
    code_and_metadata_size: 0,
    bytecode_and_metadata_size: 0,
    external_script_source_size: 0,
    cpu_profiler_metadata_size: 0
  }),
  takeCoverage: () => undefined,
  stopCoverage: () => undefined,
  writeHeapSnapshot: () => {
    const error = new Error("Heap snapshots are not supported by quench-node");
    error.code = "ERR_V8_NOT_SUPPORTED";
    throw error;
  }
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "v8") {
    return Object.assign(
      {},
      __quenchOriginalRequireWithV8(specifier),
      __quenchV8
    );
  }
  return __quenchOriginalRequireWithV8(specifier);
};
