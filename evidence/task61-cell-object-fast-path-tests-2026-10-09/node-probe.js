'use strict';
const results = [];
function attempt(name, factory) {
  let value;
  try { value = factory(); }
  catch (error) {
    results.push({ name, creation: 'throws', error: error && error.name || typeof error, message: String(error && error.message || error) });
    return;
  }
  if (value === undefined) {
    results.push({ name, creation: 'unavailable' });
    return;
  }
  const key = `__cell_probe_${name.replace(/[^a-zA-Z0-9]/g, '_')}__`;
  const record = { name, creation: 'ok', tag: Object.prototype.toString.call(value) };
  try {
    const before = Object.getPrototypeOf(value);
    const set = Reflect.set(value, key, 17);
    const first = Reflect.get(value, key);
    const own = Object.prototype.hasOwnProperty.call(value, key);
    Object.defineProperty(value, key, { value: 23, configurable: true, enumerable: false, writable: true });
    const descriptor = Object.getOwnPropertyDescriptor(value, key);
    Reflect.set(value, key, 29);
    const updated = Reflect.get(value, key);
    const deleted = Reflect.deleteProperty(value, key);
    const absent = !Object.prototype.hasOwnProperty.call(value, key);
    const proto = { probeInherited: name };
    Object.setPrototypeOf(value, proto);
    const inherited = Reflect.get(value, 'probeInherited');
    Object.setPrototypeOf(value, before);
    record.operations = { set, first, own, descriptorValue: descriptor && descriptor.value, enumerable: descriptor && descriptor.enumerable, updated, deleted, absent, inherited, prototypeRestored: Object.getPrototypeOf(value) === before };
  } catch (error) {
    record.operationsError = { name: error && error.name || typeof error, message: String(error && error.message || error) };
  }
  results.push(record);
}

attempt('object', () => ({}));
attempt('array', () => [1, 2]);
attempt('function', () => function cellProbe() {});
attempt('async-function', () => async function cellProbeAsync() {});
attempt('generator-function', () => function* cellProbeGenerator() {});
attempt('bound-function', () => (function cellProbeBound() {}).bind(null));
attempt('proxy', () => new Proxy({}, {}));
attempt('proxy-with-traps', () => new Proxy({}, {
  get(target, key, receiver) { if (key === '__cell_probe_proxy_with_traps__') return 31; return Reflect.get(target, key, receiver); },
  set(target, key, value, receiver) { return Reflect.set(target, key, value, receiver); },
  defineProperty(target, key, desc) { return Reflect.defineProperty(target, key, desc); },
  deleteProperty(target, key) { return Reflect.deleteProperty(target, key); }
}));
attempt('array-buffer', () => new ArrayBuffer(8));
attempt('shared-array-buffer', () => typeof SharedArrayBuffer === 'function' ? new SharedArrayBuffer(8) : undefined);
attempt('uint8-array', () => new Uint8Array(4));
attempt('float64-array', () => new Float64Array(4));
attempt('data-view', () => new DataView(new ArrayBuffer(8)));
attempt('map', () => new Map());
attempt('set', () => new Set());
attempt('weak-map', () => new WeakMap());
attempt('weak-set', () => new WeakSet());
attempt('weak-ref', () => typeof WeakRef === 'function' ? new WeakRef({}) : undefined);
attempt('finalization-registry', () => typeof FinalizationRegistry === 'function' ? new FinalizationRegistry(() => {}) : undefined);
attempt('array-iterator', () => [1, 2][Symbol.iterator]());
attempt('string-iterator', () => 'ab'[Symbol.iterator]());
attempt('map-iterator', () => new Map([[1, 2]]).entries());
attempt('set-iterator', () => new Set([1]).values());
attempt('generator-object', () => (function* () { yield 1; })());
attempt('date', () => new Date(0));
attempt('regexp', () => /a/g);
attempt('regexp-match-iterator', () => 'aa'.matchAll(/a/g));
attempt('promise', () => Promise.resolve(1));
attempt('error', () => new Error('probe'));
attempt('aggregate-error', () => typeof AggregateError === 'function' ? new AggregateError([], 'probe') : undefined);
attempt('shadow-realm', () => typeof ShadowRealm === 'function' ? new ShadowRealm() : undefined);
attempt('temporal-duration', () => typeof Temporal === 'object' ? new Temporal.Duration(0, 0, 0, 1) : undefined);
attempt('temporal-plain-date', () => typeof Temporal === 'object' ? new Temporal.PlainDate(2020, 1, 2) : undefined);
attempt('temporal-plain-datetime', () => typeof Temporal === 'object' ? new Temporal.PlainDateTime(2020, 1, 2) : undefined);
attempt('temporal-plain-monthday', () => typeof Temporal === 'object' ? new Temporal.PlainMonthDay(1, 2) : undefined);
attempt('temporal-plain-yearmonth', () => typeof Temporal === 'object' ? new Temporal.PlainYearMonth(2020, 1) : undefined);
attempt('temporal-zoneddatetime', () => typeof Temporal === 'object' ? new Temporal.ZonedDateTime(0n, 'UTC') : undefined);
attempt('temporal-instant', () => typeof Temporal === 'object' ? new Temporal.Instant(0n) : undefined);
attempt('wasm-global', () => typeof WebAssembly === 'object' ? new WebAssembly.Global({ value: 'i32', mutable: true }, 1) : undefined);
attempt('wasm-memory', () => typeof WebAssembly === 'object' ? new WebAssembly.Memory({ initial: 1 }) : undefined);
attempt('wasm-table', () => typeof WebAssembly === 'object' ? new WebAssembly.Table({ initial: 1, element: 'anyfunc' }) : undefined);
attempt('wasm-tag', () => typeof WebAssembly === 'object' && typeof WebAssembly.Tag === 'function' ? new WebAssembly.Tag({ parameters: ['i32'] }) : undefined);
attempt('wasm-exception', () => {
  if (typeof WebAssembly !== 'object' || typeof WebAssembly.Tag !== 'function' || typeof WebAssembly.Exception !== 'function') return undefined;
  return new WebAssembly.Exception(new WebAssembly.Tag({ parameters: ['i32'] }), [1]);
});
attempt('wasm-exported-function', () => {
  if (typeof WebAssembly !== 'object') return undefined;
  const bytes = Uint8Array.from([0,97,115,109,1,0,0,0,1,4,1,96,0,0,3,2,1,0,7,7,1,3,114,117,110,0,0,10,4,1,2,0,11]);
  return new WebAssembly.Instance(new WebAssembly.Module(bytes)).exports.run;
});

const primitives = [undefined, null, false, 0, -0, 3.5, 'abc', 1n, Symbol('probe')];
const primitiveResults = [];
for (const value of primitives) {
  let label;
  try { label = value === null ? 'null' : typeof value === 'number' && Object.is(value, -0) ? '-0' : typeof value; } catch (_) { label = 'unknown'; }
  try {
    const property = value.toString;
    primitiveResults.push({ label, read: typeof property, stringValue: property.call(value) });
  } catch (error) {
    primitiveResults.push({ label, throws: error && error.name || typeof error });
  }
}
results.push({ name: 'non-object-cells', operations: primitiveResults });
console.log(JSON.stringify(results));
