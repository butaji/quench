'use strict';
// v8 serialization contract: the exact bytes v8.serialize writes, what v8.deserialize and
// custom Serializer/Deserializer subclasses read back, and the errors on both paths.
const v8 = require('v8');
const util = require('util');

const show = (value) => util.inspect(value, { depth: 6, breakLength: Infinity, showHidden: false });
const hex = (value) => Buffer.from(v8.serialize(value)).toString('hex');
function observe(label, run) {
  try {
    console.log(label, run());
  } catch (error) {
    console.log(label, 'throws', error.constructor.name, error.name, error.code ?? '', error.message);
  }
}

const sparse = [1, , 3];
sparse.extra = 'x';
const dense = [1, 'two', { three: 3 }];
dense.named = true;
const cyclic = { name: 'cycle' };
cyclic.self = cyclic;
const shared = { shared: true };
const error = new RangeError('bad range', { cause: { code: 42 } });
error.stack = 'RangeError: bad range\n    at fixture';
const plainError = new Error('plain');
plainError.stack = 'Error: plain';
const custom = new TypeError('custom name');
custom.name = 'CustomError';
custom.stack = 'stack';
const resizable = new ArrayBuffer(4, { maxByteLength: 16 });
new Uint8Array(resizable).set([1, 2, 3, 4]);

const values = {
  undefined: undefined, null: null, true: true, false: false,
  int: 42, negative: -7, smiMax: 2 ** 31 - 1, beyondSmi: 2 ** 31, negativeZero: -0, fraction: 1.5,
  nan: NaN, infinity: -Infinity,
  bigint: 12345678901234567890n, negativeBigint: -(2n ** 64n), zeroBigint: 0n,
  ascii: 'hello', latin1: 'café', twoByte: 'snow ☃', twoBytePadded: 'x☃', empty: '',
  loneSurrogate: '\ud800',
  object: { a: 1, b: 'two', 3: 'three', [Symbol('hidden')]: 1 },
  nested: { list: [1, [2, [3]]], map: new Map([[1, 'one'], ['k', { v: true }]]), set: new Set(['a', 1, null]) },
  dense, sparse, holes: new Array(3), filled: new Array(3).fill(0), doubles: [1.5, 2, 3], mixedNumbers: [1, 2 ** 40],
  cyclic, aliases: [shared, shared, { again: shared }],
  date: new Date(1700000000000), invalidDate: new Date(NaN),
  regexp: /a+b/gimsuyd, regexpSets: /[\p{L}]/v,
  boxed: [Object(true), Object(false), Object(3.5), Object(7n), Object('str')],
  error, plainError, custom,
  arrayBuffer: new Uint8Array([1, 2, 3]).buffer, resizable,
  buffer: Buffer.from('buffer'), typed: new Float64Array([0.5, -1]), view: new DataView(new ArrayBuffer(4), 1, 2),
  bigints: new BigInt64Array([1n, -1n]), clamped: new Uint8ClampedArray([300 & 255]),
  viewInObject: { first: new Uint16Array([1, 2]), second: new Int8Array([-1]) },
};
for (const [name, value] of Object.entries(values)) {
  observe(`serialize ${name}`, () => hex(value));
  observe(`round-trip ${name}`, () => show(v8.deserialize(v8.serialize(value))));
}
const errorCopy = v8.deserialize(v8.serialize(error));
console.log('error copy', errorCopy instanceof RangeError, errorCopy.message, show(errorCopy.cause), errorCopy.stack,
  Object.getOwnPropertyDescriptor(errorCopy, 'message').enumerable, Object.getOwnPropertyDescriptor(errorCopy, 'cause').enumerable);
const customCopy = v8.deserialize(v8.serialize(custom));
console.log('custom error copy', customCopy.constructor.name, customCopy.name, customCopy.message);

for (const [name, value] of Object.entries({
  function: () => 1, symbol: Symbol('s'), promise: Promise.resolve(), weakMap: new WeakMap(),
  proxy: new Proxy({}, {}), sharedArrayBuffer: new SharedArrayBuffer(4), boxedSymbol: Object(Symbol('b')),
  args: (function () { return arguments; })(1), nestedFunction: { inner: [function named() {}] },
})) {
  observe(`reject ${name}`, () => hex(value));
}

const getterOrder = [];
observe('getters run in key order', () => hex({
  get a() { getterOrder.push('a'); return 1; },
  get b() { getterOrder.push('b'); delete this.c; return 2; },
  c: 3,
}));
console.log('getter order', getterOrder.join());

for (const [name, bytes] of Object.entries({
  truncated: 'ff0f6f2201',
  badTag: 'ff0f99',
  futureVersion: 'ff10',
  danglingReference: 'ff0f5e05',
  countMismatch: 'ff0f6f7b05',
  duplicateKey: 'ff0f6f2201614901220161490a7b02',
  noHeader: '4904',
  legacyUndefinedHole: 'ff0a41025f5f240002',
  verifyCount: 'ff0f3f0249 08'.replace(/ /g, ''),
  padding: 'ff0f00004904',
  utf8String: 'ff0f5304f09f9880',
  uint32: 'ff0f55ffffffff0f',
  badView: 'ff0f4202000056420004',
  regexpBadFlags: 'ff0f5222016180800c',
})) {
  observe(`deserialize ${name}`, () => show(v8.deserialize(Buffer.from(bytes, 'hex'))));
}

class HostSerializer extends v8.Serializer {
  _writeHostObject(object) {
    this.writeUint32(object.id);
    this.writeUint64(0x12345678, 0x9abcdef0);
    this.writeDouble(object.weight);
    this.writeRawBytes(Buffer.from(object.tag));
  }
  _getSharedArrayBufferId() { return 7; }
}
class HostDeserializer extends v8.Deserializer {
  _readHostObject() {
    const id = this.readUint32();
    const [hi, lo] = this.readUint64();
    const weight = this.readDouble();
    const tag = this.readRawBytes(3).toString();
    return { restored: id, hi, lo, weight, tag };
  }
}
const serializer = new HostSerializer();
serializer.writeHeader();
serializer._setTreatArrayBufferViewsAsHostObjects(true);
const host = new Uint8Array(2);
Object.assign(host, {});
const hostObject = Object.assign(new Float32Array(1), {});
host.id = 5;
host.weight = 0.25;
host.tag = 'abc';
observe('host write', () => serializer.writeValue({ host, again: host, sab: new SharedArrayBuffer(8) }));
const written = serializer.releaseBuffer();
console.log('host bytes', written.toString('hex'), Buffer.isBuffer(written), hostObject.length);
const deserializer = new HostDeserializer(written);
console.log('header', deserializer.readHeader(), deserializer.getWireFormatVersion());
observe('host read', () => show(deserializer.readValue()));

const transfer = new v8.Serializer();
transfer.writeHeader();
const transferred = new ArrayBuffer(4);
transfer.transferArrayBuffer(3, transferred);
transfer.writeValue({ transferred, view: new Uint8Array(transferred, 1, 2) });
const transferBytes = transfer.releaseBuffer();
console.log('transfer bytes', transferBytes.toString('hex'));
const receiver = new v8.Deserializer(transferBytes);
receiver.readHeader();
const replacement = new Uint8Array([9, 8, 7, 6]).buffer;
receiver.transferArrayBuffer(3, replacement);
const received = receiver.readValue();
console.log('transfer read', received.transferred === replacement, show(received.view));
const missing = new v8.Deserializer(transferBytes);
missing.readHeader();
observe('transfer missing', () => missing.readValue());

const plain = new v8.Serializer();
plain.writeHeader();
observe('plain host object', () => plain.writeValue(new Uint8Array(1)));
observe('plain view', () => { plain._setTreatArrayBufferViewsAsHostObjects(true); return plain.writeValue(new Uint8Array(1)); });
observe('plain sab', () => new v8.Serializer().writeValue(new SharedArrayBuffer(1)));
observe('transfer non-buffer', () => new v8.Serializer().transferArrayBuffer(1, {}));
observe('deserializer argument', () => new v8.Deserializer('nope'));
observe('raw bytes argument', () => new v8.Serializer().writeRawBytes('nope'));
const reader = new v8.Deserializer(Buffer.from('ff0f', 'hex'));
reader.readHeader();
observe('read past end', () => reader.readUint32());
observe('read value past end', () => reader.readValue());
observe('read host without hook', () => {
  const bytes = Buffer.from('ff0f5c01', 'hex');
  const d = new v8.Deserializer(bytes);
  d.readHeader();
  return d.readValue();
});
observe('read host non-object', () => {
  class Bad extends v8.Deserializer { _readHostObject() { return 1; } }
  const d = new Bad(Buffer.from('ff0f5c', 'hex'));
  d.readHeader();
  return d.readValue();
});
observe('data clone hook', () => {
  class Hooked extends v8.Serializer { _getDataCloneError(message) { return new SyntaxError(`hooked: ${message}`); } }
  const s = new Hooked();
  s.writeHeader();
  return s.writeValue(Symbol('x'));
});

const statistics = v8.getHeapStatistics();
console.log('heap statistics keys', Object.keys(statistics).join(), Object.values(statistics).every(Number.isFinite));
console.log('heap limit dominates used', statistics.heap_size_limit > statistics.used_heap_size);
console.log('heap spaces', v8.getHeapSpaceStatistics().map((space) => space.space_name).join());
console.log('code statistics', Object.keys(v8.getHeapCodeStatistics()).join());
console.log('version tag', typeof v8.cachedDataVersionTag(), Number.isInteger(v8.cachedDataVersionTag()));
console.log('one byte', v8.isStringOneByteRepresentation('abc'), v8.isStringOneByteRepresentation('☃'));
const profiler = new v8.GCProfiler();
profiler.start();
const profile = profiler.stop();
console.log('gc profile', Object.keys(profile).join(), profile.version, Array.isArray(profile.statistics));
console.log('flags', v8.setFlagsFromString('--allow-natives-syntax'));
console.log('startup snapshot', v8.startupSnapshot.isBuildingSnapshot());
