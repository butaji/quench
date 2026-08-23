//! Polyfill: `v8`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithV8 = globalThis.require;
const __quenchV8 = {
  serialize(value) {
    const text = JSON.stringify(value, (key, item) => {
      if (item && item.type === "Buffer" && Array.isArray(item.data)) {
        return { __quench_type: "buffer", data: item.data };
      }
      return item;
    });
    return Buffer.from(text);
  },
  deserialize(value) {
    const text = Buffer.isBuffer(value) ? value.toString() : String(value);
    return JSON.parse(text, (key, item) =>
      item && item.__quench_type === "buffer" ? Buffer.from(item.data) : item
    );
  },
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
    does_zap_garbage: 0,
  }),
  getHeapSpaceStatistics: () => [
    {
      space_name: "read_only_space",
      space_size: 0,
      space_used_size: 0,
      space_available_size: 0,
      physical_space_size: 0,
    },
  ],
  getHeapCodeStatistics: () => ({
    code_and_metadata_size: 0,
    bytecode_and_metadata_size: 0,
    external_script_source_size: 0,
    cpu_profiler_metadata_size: 0,
  }),
  takeCoverage: () => undefined,
  stopCoverage: () => undefined,
  writeHeapSnapshot: () => {
    const error = new Error("Heap snapshots are not supported by quench-node");
    error.code = "ERR_V8_NOT_SUPPORTED";
    throw error;
  },
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "v8") {
    return Object.assign(
      {},
      __quenchOriginalRequireWithV8(specifier),
      __quenchV8,
    );
  }
  return __quenchOriginalRequireWithV8(specifier);
};
"#);
