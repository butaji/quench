// V8 compatibility surface backed by the embedded engine.
function encode(value) {
  return JSON.stringify(value, (key, item) => {
    if (item === undefined) return { __quench_type: 'undefined' };
    if (item instanceof Uint8Array) return { __quench_type: 'uint8array', data: Array.from(item) };
    return item;
  });
}
function decode(text) {
  return JSON.parse(text, (key, item) => {
    if (item && item.__quench_type === 'undefined') return undefined;
    if (item && item.__quench_type === 'uint8array') return new Uint8Array(item.data);
    return item;
  });
}
function serialize(value) { return Buffer.from(encode(value)); }
function deserialize(value) {
  const bytes = Buffer.isBuffer(value) ? value.toString() : String(value);
  return decode(bytes);
}
class Serializer {
  writeValue(value) { this._value = serialize(value); return true; }
  releaseBuffer() { return this._value || Buffer.alloc(0); }
}
class DefaultSerializer extends Serializer {}
class Deserializer {
  constructor(value) { this._value = value; }
  readValue() { return deserialize(this._value); }
}
class DefaultDeserializer extends Deserializer {}
module.exports = {
  Serializer, DefaultSerializer, Deserializer, DefaultDeserializer,
  cachedDataVersionTag() { return 0; },
  getHeapStatistics() {
    return {
      total_heap_size: 1, total_heap_size_executable: 0, total_physical_size: 1,
      total_available_size: 1, used_heap_size: 1, heap_size_limit: 1,
      malloced_memory: 0, external_memory: 0, peak_malloced_memory: 0,
      does_zap_garbage: 0, number_of_native_contexts: 1,
      number_of_detached_contexts: 0, total_global_handles_size: 0,
      used_global_handles_size: 0
    };
  },
  getHeapSpaceStatistics() {
    return [{
      space_name: 'read_only_space',
      space_size: 0,
      space_used_size: 0,
      space_available_size: 0,
      physical_space_size: 0
    }];
  },
  getHeapCodeStatistics() {
    return { code_and_metadata_size: 0, bytecode_and_metadata_size: 0,
      external_script_source_size: 0, cpu_profiler_metadata_size: 0 };
  },
  getHeapSnapshot() { throw new Error('v8.getHeapSnapshot is unavailable in the embedded runtime'); },
  writeHeapSnapshot(file) {
    const fs = require('node:fs');
    const name = String(file || 'quench.heapsnapshot');
    fs.writeFileSync(name, Buffer.from('QuenchNode-heap-snapshot\n', 'utf8'));
    return name;
  },
  getVersion() { return 'v8-embedded'; },
  setFlagsFromString() {},
  serialize, deserialize,
  promiseHooks: { createHook() { return { enable() {}, disable() {} }; } }
};
