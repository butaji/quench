use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const MODULE: &str = r#"((queryObjectsNative) => {
  const flags = new Set();
  function cachedDataVersionTag() {
    let hash = 0x811c9dc5;
    const values = [String(process.versions?.v8 || 'quench'), ...[...flags].sort()];
    for (const value of values) {
      for (let i = 0; i < value.length; i++) {
        hash = Math.imul(hash ^ value.charCodeAt(i), 0x01000193);
      }
      hash = Math.imul(hash ^ 0xff, 0x01000193);
    }
    return hash >>> 0;
  }
  function setFlagsFromString(value) {
    for (const flag of String(value).trim().split(/\s+/)) {
      if (flag) flags.add(flag);
    }
  }
  function getCppHeapStatistics(type = 'detailed') {
    if (type !== 'brief' && type !== 'detailed') {
      const error = new TypeError(`The value of "type" is invalid. Received ${type}`);
      error.code = 'ERR_INVALID_ARG_VALUE';
      throw error;
    }
    // Quench does not currently expose V8's CppHeap, so report an empty heap
    // while preserving the public Node API's result shape.
    return {
      committed_size_bytes: 0,
      resident_size_bytes: 0,
      used_size_bytes: 0,
      detail_level: type,
      space_statistics: [],
      type_names: [],
    };
  }
  // Expose the stable shape of Node's heap-inspection APIs on engines that do
  // not provide V8's internal heap statistics. Use the host's memory counters
  // where available and zero-valued measurements for V8-specific spaces.
  function getHeapStatistics() {
    const usage = process.memoryUsage();
    const total = Number(usage.heapTotal) || 0;
    const used = Number(usage.heapUsed) || 0;
    const limit = Number.MAX_SAFE_INTEGER;
    return {
      does_zap_garbage: 0,
      external_memory: Number(usage.external) || 0,
      heap_size_limit: limit,
      malloced_memory: 0,
      number_of_detached_contexts: 0,
      number_of_native_contexts: 1,
      peak_malloced_memory: 0,
      total_allocated_bytes: used,
      total_available_size: Math.max(0, limit - used),
      total_global_handles_size: 0,
      total_heap_size: total,
      total_heap_size_executable: 0,
      total_physical_size: total,
      used_global_handles_size: 0,
      used_heap_size: used,
    };
  }
  function getHeapCodeStatistics() {
    return {
      bytecode_and_metadata_size: 0,
      code_and_metadata_size: 0,
      cpu_profiler_metadata_size: 0,
      external_script_source_size: 0,
    };
  }
  const heapSpaceNames = [
    'code_large_object_space', 'code_space', 'large_object_space',
    'new_large_object_space', 'new_space', 'old_space', 'read_only_space',
    'shared_large_object_space', 'shared_space',
    'shared_trusted_large_object_space', 'shared_trusted_space',
    'trusted_large_object_space', 'trusted_space',
  ];
  function getHeapSpaceStatistics() {
    return heapSpaceNames.map((space_name) => ({
      space_name,
      space_size: 0,
      space_used_size: 0,
      space_available_size: 0,
      physical_space_size: 0,
    }));
  }
  function isStringOneByteRepresentation(content) {
    if (typeof content !== 'string') {
      throw new TypeError(`The "content" argument must be of type string. Received ${typeof content}`);
    }
    for (let index = 0; index < content.length; index++) {
      if (content.charCodeAt(index) > 0xff) return false;
    }
    return true;
  }
  // Node's test runner uses the V8 serializer framing API for its internal
  // reporter channel. Keep the framing self-contained so the bundled test
  // runner can round-trip report records even where Quench does not expose
  // V8's native serialization format.
  const kSerializerHeader = Buffer.from([0x51, 0x55, 0x45, 0x4e, 0x43, 0x48]);
  const kSerializedValue = '\0quench-v8-value';
  const kSerializedGraph = '\0quench-v8-graph';
  function assertSerializable(value, seen = new Set()) {
    if (typeof value === 'function' || typeof value === 'symbol') {
      throw new Error('Unserializable value');
    }
    if (value === null || typeof value !== 'object' || Buffer.isBuffer(value)) return;
    if (value instanceof WeakMap || value instanceof WeakSet) {
      throw new Error('Unserializable value');
    }
    if (seen.has(value)) return;
    seen.add(value);
    if (value instanceof Map) {
      for (const [key, entry] of value) {
        assertSerializable(key, seen);
        assertSerializable(entry, seen);
      }
      return;
    }
    if (value instanceof Set) {
      for (const entry of value) assertSerializable(entry, seen);
      return;
    }
    for (const key of Object.keys(value)) {
      const descriptor = Object.getOwnPropertyDescriptor(value, key);
      if (Object.prototype.hasOwnProperty.call(descriptor, 'value')) {
        assertSerializable(descriptor.value, seen);
      }
    }
  }
  function stringifyValue(value) {
    // Match V8's failure mode for functions, symbols, and weak collections.
    // Node's error_serdes catches these failures and repairs cloneable error
    // properties from the original object, dropping those unsupported values.
    assertSerializable(value);
    const objects = [];
    const seen = new Map();
    function encode(item) {
      if (item === undefined) return { [kSerializedValue]: 'undefined' };
      if (typeof item === 'bigint') {
        return { [kSerializedValue]: 'bigint', value: String(item) };
      }
      if (item === null || typeof item !== 'object') return item;
      if (seen.has(item)) return { [kSerializedValue]: 'reference', id: seen.get(item) };

      const id = objects.length;
      seen.set(item, id);
      objects.push(null);
      if (Buffer.isBuffer(item)) {
        objects[id] = { type: 'buffer', value: Array.from(item) };
      } else if (item instanceof Map) {
        objects[id] = { type: 'map', entries: Array.from(item, ([key, entry]) => [encode(key), encode(entry)]) };
      } else if (item instanceof Set) {
        objects[id] = { type: 'set', entries: Array.from(item, encode) };
      } else if (Array.isArray(item)) {
        objects[id] = { type: 'array', entries: item.map(encode) };
      } else if (item instanceof Date) {
        objects[id] = { type: 'date', value: item.getTime() };
      } else {
        const properties = [];
        for (const key of Object.keys(item)) {
          const descriptor = Object.getOwnPropertyDescriptor(item, key);
          if (descriptor && Object.prototype.hasOwnProperty.call(descriptor, 'value')) {
            properties.push([key, encode(descriptor.value)]);
          }
        }
        objects[id] = { type: 'object', properties };
      }
      return { [kSerializedValue]: 'reference', id };
    }
    return JSON.stringify({
      [kSerializedGraph]: 1,
      root: encode(value),
      objects,
    });
  }
  function parseValue(text) {
    const graph = JSON.parse(text);
    if (graph === null || typeof graph !== 'object' || graph[kSerializedGraph] !== 1) {
      return graph;
    }
    const values = graph.objects.map((node) => {
      switch (node.type) {
        case 'array': return [];
        case 'buffer': return Buffer.from(node.value);
        case 'date': return new Date(node.value);
        case 'map': return new Map();
        case 'set': return new Set();
        default: return {};
      }
    });
    function decode(item) {
      if (item === null || typeof item !== 'object') return item;
      switch (item[kSerializedValue]) {
        case 'undefined': return undefined;
        case 'bigint': return BigInt(item.value);
        case 'reference': return values[item.id];
      }
      return item;
    }
    for (let id = 0; id < graph.objects.length; id++) {
      const node = graph.objects[id];
      const value = values[id];
      if (node.type === 'array') {
        for (const entry of node.entries) value.push(decode(entry));
      } else if (node.type === 'map') {
        for (const [key, entry] of node.entries) value.set(decode(key), decode(entry));
      } else if (node.type === 'set') {
        for (const entry of node.entries) value.add(decode(entry));
      } else if (node.type === 'object') {
        for (const [key, entry] of node.properties) {
          Object.defineProperty(value, key, {
            value: decode(entry), enumerable: true, configurable: true, writable: true,
          });
        }
      }
    }
    return decode(graph.root);
  }
  const writeVarint = (value) => {
    const bytes = [];
    value = Number(value) >>> 0;
    while (value > 0x7f) {
      bytes.push((value & 0x7f) | 0x80);
      value >>>= 7;
    }
    bytes.push(value);
    return Buffer.from(bytes);
  };
  class Serializer {
    constructor() {
      this._parts = [];
      this._hostTagParts = [];
      this._hostValueParts = [];
      this._usesCustomHostObject = false;
    }
    writeHeader() { this._parts.push(kSerializerHeader); }
    writeRawBytes(bytes) { this._parts.push(Buffer.from(bytes)); }
    writeUint32(value) {
      const bytes = writeVarint(value);
      (this._usesCustomHostObject ? this._hostTagParts : this._parts).push(bytes);
    }
    _writeHostObject(value) {
      if (!Buffer.isBuffer(value)) {
        throw new Error('Unserializable host object');
      }
      // V8's Buffer host-object payload is a tag, byte length, then bytes.
      this._hostValueParts.push(Buffer.from([0x0a]), writeVarint(value.length), value);
    }
    writeValue(value) {
      // Node's error_serdes falls back to v8.serialize() for DOMException,
      // whose host-object prototype is not part of its builtin Error list.
      // Preserve the observable fields so benchmark error records retain the
      // DataCloneError name when reconstructed in the shared VM.
      if (typeof DOMException === 'function' && value instanceof DOMException) {
        this._parts.push(Buffer.from(stringifyValue({
          name: value.name,
          message: value.message,
          code: value.code,
        }), 'utf8'));
        return;
      }
      if (Buffer.isBuffer(value) &&
          this._writeHostObject !== Serializer.prototype._writeHostObject) {
        this._usesCustomHostObject = true;
        this._writeHostObject(value);
        return;
      }
      this._parts.push(Buffer.from(stringifyValue(value), 'utf8'));
    }
    releaseBuffer() {
      const value = this._usesCustomHostObject
        ? Buffer.concat([
          Buffer.from([0xff, 0x0f, 0x5c]),
          ...this._hostTagParts,
          ...this._hostValueParts,
        ])
        : Buffer.concat(this._parts);
      this._parts = [];
      this._hostTagParts = [];
      this._hostValueParts = [];
      this._usesCustomHostObject = false;
      return value;
    }
  }
  class Deserializer {
    constructor(bytes) { this._bytes = Buffer.from(bytes); this._offset = 0; }
    readHeader() {
      const end = this._offset + kSerializerHeader.length;
      if (end > this._bytes.length || !this._bytes.subarray(this._offset, end).equals(kSerializerHeader)) {
        throw new Error('Unable to deserialize data');
      }
      this._offset = end;
      return true;
    }
    readValue() {
      return parseValue(this._bytes.subarray(this._offset).toString('utf8'));
    }
  }
  function serialize(value) {
    const serializer = new Serializer();
    serializer.writeHeader();
    serializer.writeValue(value);
    return serializer.releaseBuffer();
  }
  function deserialize(bytes) {
    const deserializer = new Deserializer(bytes);
    deserializer.readHeader();
    return deserializer.readValue();
  }
  let queryObjectsWarningEmitted = false;
  function queryObjects(ctor, options = {}) {
    if (typeof ctor !== 'function') {
      const error = new TypeError('The "ctor" argument must be of type Function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (options === null || typeof options !== 'object') {
      const error = new TypeError('The "options" argument must be of type Object');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    const format = options.format || 'count';
    if (format !== 'count' && format !== 'summary') {
      const error = new TypeError(`The value of "options.format" is invalid. Received ${format}`);
      error.code = 'ERR_INVALID_ARG_VALUE';
      throw error;
    }
    if (!queryObjectsWarningEmitted) {
      queryObjectsWarningEmitted = true;
      process.emitWarning('v8.queryObjects() is an experimental feature and might change at any time', 'ExperimentalWarning');
    }
    const objects = queryObjectsNative(ctor.prototype);
    if (format === 'count') return objects.length;
    const inspect = process.getBuiltinModule('util').inspect;
    return objects.map((object) => inspect(object, { depth: 0 }));
  }
  const promiseHookRegistry = {
    init: new Set(),
    before: new Set(),
    after: new Set(),
    settled: new Set(),
  };
  const promiseHookNames = ['init', 'before', 'after', 'settled'];
  function validatePromiseHook(name, callback) {
    const source = typeof callback === 'function' ? Function.prototype.toString.call(callback) : '';
    if (typeof callback !== 'function' || /^async(?:\s|\*)/.test(source)) {
      throw new TypeError(`The "${name}Hook" argument must be of type function`);
    }
  }
  function subscribePromiseHook(name, callback) {
    validatePromiseHook(name, callback);
    const hooks = promiseHookRegistry[name];
    hooks.add(callback);
    let active = true;
    return function stopPromiseHook() {
      if (!active) return;
      active = false;
      hooks.delete(callback);
    };
  }
  const promiseHooks = {
    onInit(callback) { return subscribePromiseHook('init', callback); },
    onBefore(callback) { return subscribePromiseHook('before', callback); },
    onAfter(callback) { return subscribePromiseHook('after', callback); },
    onSettled(callback) { return subscribePromiseHook('settled', callback); },
    createHook(options) {
      if (options === null || typeof options !== 'object') {
        throw new TypeError('The "options" argument must be of type object');
      }
      const callbacks = [];
      for (const name of promiseHookNames) {
        const callback = options[name];
        if (callback !== undefined) {
          validatePromiseHook(name, callback);
          callbacks.push([name, callback]);
        }
      }
      for (const [name, callback] of callbacks) promiseHookRegistry[name].add(callback);
      let active = true;
      return function stopPromiseHooks() {
        if (!active) return;
        active = false;
        for (const [name, callback] of callbacks) promiseHookRegistry[name].delete(callback);
      };
    },
  };
  promiseHookRegistry.emit = function emitPromiseHook(name, ...args) {
    const callbacks = Array.from(promiseHookRegistry[name]);
    for (const callback of callbacks) {
      try {
        Reflect.apply(callback, undefined, args);
      } catch (error) {
        process.emit('uncaughtException', error);
      }
    }
  };
  function snapshotNotBuildingError() {
    const error = new Error('Startup snapshot is not being built');
    error.code = 'ERR_NOT_BUILDING_SNAPSHOT';
    return error;
  }
  const exports = {
    DefaultDeserializer: Deserializer,
    DefaultSerializer: Serializer,
    Deserializer,
    Serializer,
    cachedDataVersionTag,
    getCppHeapStatistics,
    getHeapStatistics,
    getHeapCodeStatistics,
    getHeapSpaceStatistics,
    isStringOneByteRepresentation,
    queryObjects,
    promiseHooks,
    serialize,
    deserialize,
    setFlagsFromString,
    startupSnapshot: Object.freeze({
      isBuildingSnapshot() { return false; },
      addSerializeCallback() { throw snapshotNotBuildingError(); },
      addDeserializeCallback() { throw snapshotNotBuildingError(); },
      setDeserializeMainFunction() { throw snapshotNotBuildingError(); },
    }),
  };
  return [exports, promiseHookRegistry];
})"#;

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(MODULE, "node:v8/shared-vm.js")?;
    let native = context.host_function(crate::host::shared_vm::operation("v8QueryObjects"))?;
    let undefined = context.undefined();
    let tuple = context.call_rooted(factory, undefined, &[native])?;
    let module_key = context.string_rooted("0");
    let module = context.get_property_rooted(tuple, module_key)?;
    context.release_root(module_key);
    let registry_key = context.string_rooted("1");
    let registry = context.get_property_rooted(tuple, registry_key)?;
    context.release_root(registry_key);
    let result = super::promise_hooks_shared_vm::bind_registry(context, registry);
    for root in [registry, tuple, factory, native] {
        context.release_root(root);
    }
    result.map(|()| module)
}

pub(crate) fn query_objects(
    context: &mut NativeContext<'_, NodeHost>,
    _receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let prototype = args.first().copied().unwrap_or_else(|| context.undefined());
    let objects = context.objects_with_prototype_rooted(prototype)?;
    context.array_rooted(&objects)
}
