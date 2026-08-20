// V8 compatibility surface backed by the embedded engine.
function serialize(value) {
  return Buffer.from(JSON.stringify(value));
}
function deserialize(value) {
  const bytes = Buffer.isBuffer(value) ? value.toString() : String(value);
  return JSON.parse(bytes);
}
module.exports = {
  cachedDataVersionTag() { return 0; },
  getHeapStatistics() {
    return {
      total_heap_size: 0, total_heap_size_executable: 0, total_physical_size: 0,
      total_available_size: 0, used_heap_size: 0, heap_size_limit: 0,
      malloced_memory: 0, external_memory: 0, peak_malloced_memory: 0,
      does_zap_garbage: 0, number_of_native_contexts: 0,
      number_of_detached_contexts: 0, total_global_handles_size: 0,
      used_global_handles_size: 0
    };
  },
  getHeapSpaceStatistics() { return []; },
  getHeapCodeStatistics() {
    return { code_and_metadata_size: 0, bytecode_and_metadata_size: 0,
      external_script_source_size: 0, cpu_profiler_metadata_size: 0 };
  },
  getHeapSnapshot() { throw new Error('v8.getHeapSnapshot is unavailable in the embedded runtime'); },
  getVersion() { return 'v8-embedded'; },
  setFlagsFromString() {},
  serialize, deserialize,
  promiseHooks: { createHook() { return { enable() {}, disable() {} }; } }
};
