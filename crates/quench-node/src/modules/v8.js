module.exports = {
  cachedDataVersionTag() { return 0; },
  getHeapStatistics() { return { total_heap_size: 0, used_heap_size: 0, heap_size_limit: 0 }; },
  getHeapSpaceStatistics() { return []; },
  getHeapCodeStatistics() { return { code_and_metadata_size: 0, bytecode_and_metadata_size: 0, external_script_source_size: 0 }; },
  getVersion() { return 'v8-embedded'; },
  setFlagsFromString() {},
  promiseHooks: { createHook() { return { enable() {}, disable() {} }; } }
};
