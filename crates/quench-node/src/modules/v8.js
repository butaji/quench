// V8 compatibility surface backed by the embedded engine.
function encode(value) {
  return JSON.stringify(value, (key, item) => {
    if (typeof item === 'bigint') {
      return { __quench_type: 'bigint', value: item.toString() };
    }
    if (item === undefined) return { __quench_type: 'undefined' };
    // Buffer's toJSON hook runs before this replacer, so recognize its
    // canonical `{ type: "Buffer", data: [...] }` representation explicitly.
    if (item && item.type === 'Buffer' && Array.isArray(item.data)) {
      return { __quench_type: 'buffer', data: item.data };
    }
    if (item instanceof Uint8Array) {
      return { __quench_type: 'uint8array', data: Array.from(item) };
    }
    return item;
  });
}
function decode(text) {
  return JSON.parse(text, (key, item) => {
    if (item && item.__quench_type === 'undefined') return undefined;
    if (item && item.__quench_type === 'bigint') return BigInt(item.value);
    if (item && item.__quench_type === 'buffer') return Buffer.from(item.data);
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
    // Keep the public V8 shape useful even though the embedded runtime does
    // not expose V8's per-space allocator. These values are process-local and
    // therefore track allocations instead of returning misleading constants.
    const memory = process.memoryUsage();
    const heapTotal = Number(memory.heapTotal) || 0;
    const heapUsed = Math.min(heapTotal, Number(memory.heapUsed) || 0);
    const external = Number(memory.external) || 0;
    const totalAllocated = heapTotal + external;
    return {
      total_heap_size: heapTotal,
      total_heap_size_executable: 0,
      total_physical_size: heapTotal,
      total_allocated_bytes: totalAllocated,
      total_available_size: Math.max(0, 4 * 1024 * 1024 * 1024 - heapUsed),
      used_heap_size: heapUsed,
      heap_size_limit: 4 * 1024 * 1024 * 1024,
      malloced_memory: 0,
      external_memory: external,
      peak_malloced_memory: 0,
      does_zap_garbage: 0,
      number_of_native_contexts: 1,
      number_of_detached_contexts: 0,
      total_global_handles_size: 0,
      used_global_handles_size: 0
    };
  },
  getHeapSpaceStatistics() {
    const memory = process.memoryUsage();
    const total = Number(memory.heapTotal) || 0;
    const used = Math.min(Number(memory.heapUsed) || 0, total);
    // Node exposes a version-dependent list, but always returns objects with
    // these five fields. Use stable coarse spaces for the embedded engine.
    const spaces = ['read_only_space', 'new_space', 'old_space', 'code_space'];
    return spaces.map((space_name, index) => {
      const space_size = index === 0 ? 0 : Math.floor(total / (spaces.length - 1));
      const space_used_size = index === 0 ? 0 : Math.min(
        space_size, index === spaces.length - 1 ? 0 : Math.floor(used / 2)
      );
      return {
        space_name,
        space_size,
        space_used_size,
        space_available_size: Math.max(0, space_size - space_used_size),
        physical_space_size: space_size
      };
    });
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
  setFlagsFromString(flags) {
    if (typeof flags !== 'string') {
      const error = new TypeError('The "flags" argument must be of type string');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
  },
  serialize, deserialize,
  promiseHooks: { createHook() { return { enable() {}, disable() {} }; } }
};
