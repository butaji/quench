'use strict';
// Drives napi_contract.node (path in argv[2]) and prints one observation per line.
const addon = require(process.argv[2]);
const show = (label, value) => console.log(label, typeof value === 'string' ? value : JSON.stringify(value, (key, item) =>
  typeof item === 'symbol' ? `symbol:${item.description}` :
  item instanceof Error ? { name: item.name, message: item.message, code: item.code } :
  ArrayBuffer.isView(item) ? Array.from(new Uint8Array(item.buffer, item.byteOffset, item.byteLength)) :
  item instanceof ArrayBuffer ? Array.from(new Uint8Array(item)) : item));

show('keys', Object.keys(addon));
show('descriptor', Object.getOwnPropertyDescriptor(addon, 'types'));
show('function', [addon.types.name, addon.types.length, typeof addon.types.prototype]);
show('types', addon.types(undefined, null, true, 1, 's', Symbol('x'), {}, () => {}));
show('strings', addon.strings('héllo wörld'));
show('strings-astral', addon.strings('😀ab'));
for (const value of [3.9, -1, 2 ** 32 + 5, -(2 ** 63) * 4, NaN, Infinity, 'x']) show('numbers', addon.numbers(value));

const object = addon.properties();
show('props', [object.value, object.method(), object.accessor, object._names, object._writable,
  object._hasHidden, object._badName, object._deleted, 'hidden' in object]);
object.accessor = 5;
show('setter', object._stored);
show('prop-descriptors', ['value', 'method', 'accessor'].map((name) => {
  const descriptor = Object.getOwnPropertyDescriptor(object, name);
  return [name, descriptor.writable, descriptor.enumerable, descriptor.configurable, typeof descriptor.get, typeof descriptor.set];
}));

const { Counter } = addon;
const counter = new Counter(10);
counter.increment();
show('counter', [counter.increment(), counter.value, counter.rewrap, Counter.name, typeof Counter.finalized,
  Object.getPrototypeOf(counter) === Counter.prototype, counter instanceof Counter,
  Object.getOwnPropertyNames(Counter.prototype)]);
class Derived extends Counter { twice() { this.increment(); return this.increment(); } }
const derived = new Derived(1);
show('derived', [derived.twice(), derived instanceof Counter, derived.constructor.name]);
try { Counter(1); } catch (error) { show('call-without-new', error); }

show('errors', addon.errors(() => { throw new RangeError('from js'); }));
try { addon.throwCoded(); } catch (error) { show('thrown', [error instanceof TypeError, error.code, error.message]); }
show('references', addon.references());
show('scopes', addon.scopes());

const input = Buffer.from('abc');
const buffers = addon.buffers(input);
show('buffers', buffers);
show('buffer-classes', [Buffer.isBuffer(buffers.copy), Buffer.isBuffer(buffers.external), input.toString(),
  buffers.uint16.constructor.name, buffers.dataView.byteOffset, buffers.dataView.byteLength]);
show('externals', addon.externals());

class Base {}
show('misc', addon.misc(new (class extends Base { toString() { return '12'; } })(), Base));

addon.sum(100000).then((sum) => show('sum', sum));
const seen = [];
addon.stream((value) => {
  seen.push(value);
  if (value === 'finalized') show('stream', seen);
});

// V8 API functions are constructible; class methods carry a signature and their name.
const own = (object, key) => {
  const descriptor = Object.getOwnPropertyDescriptor(object, key);
  return descriptor && [descriptor.writable, descriptor.enumerable, descriptor.configurable];
};
const getter = Object.getOwnPropertyDescriptor(Counter.prototype, 'value').get;
show('function-shapes', [Counter.prototype.increment.name, typeof Counter.prototype.increment.prototype,
  Counter.finalized.name, typeof Counter.finalized.prototype, getter.name, typeof getter.prototype, Counter.length,
  own(Counter, 'prototype'), own(Counter.prototype, 'constructor'), own(Counter.prototype, 'increment'),
  own(addon.types, 'prototype'), own(addon.types, 'name'), Object.getOwnPropertyNames(addon.types),
  Object.getOwnPropertyNames(Counter)]);
for (const attempt of [() => Counter.prototype.increment.call({}), () => new Counter.prototype.increment(), () => typeof new addon.types()]) {
  try { show('construct-or-call', attempt()); } catch (error) { show('construct-or-call', [error.constructor.name, error.message]); }
}
