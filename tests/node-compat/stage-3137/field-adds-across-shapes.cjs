// Stores that add properties to receivers of many shapes, as schema initializers do: the
// added values, key order, setters on the receiver and its prototype, proxies, frozen,
// non-extensible and read-only receivers, and a throwing setter, with the storing function hot.
'use strict';
const assert = require('assert');

const log = [];
function init(inst, k) {
  inst.parse = (x) => x + k;
  inst.safeParse = (x) => ({ ok: true, x });
  inst.tag = k;
  return inst;
}

const shapes = [];
for (let i = 0; i < 3000; i++) {
  const kind = i % 7;
  const base = kind === 0 ? { a: 1 } : kind === 1 ? { b: 2, c: 3 } : kind === 2 ? [] : kind === 3 ? function f() {}
    : kind === 4 ? Object.create(null) : kind === 5 ? { d: 4 } : new Map();
  shapes.push(init(base, i));
}
assert.strictEqual(shapes[2999].parse(1), 3000);
assert.strictEqual(shapes[10].tag, 10);
assert.deepStrictEqual(Object.keys(shapes[1]), ['b', 'c', 'parse', 'safeParse', 'tag']);

const withSetter = { set parse(v) { log.push('setter ' + typeof v); } };
init(withSetter, 1);
assert.deepStrictEqual(Object.keys(withSetter), ['parse', 'safeParse', 'tag']);

assert.throws(() => init(Object.freeze({}), 1), TypeError);

const proxy = new Proxy({}, { set(target, key, value) { log.push('proxy ' + String(key)); target[key] = value; return true; } });
init(proxy, 2);

const readOnly = {};
Object.defineProperty(readOnly, 'parse', { value: 1, writable: false });
assert.throws(() => init(readOnly, 3), TypeError);

const throwing = { set tag(v) { throw new RangeError('boom ' + v); } };
assert.throws(() => init(throwing, 9), { name: 'RangeError', message: 'boom 9' });
assert.strictEqual(typeof throwing.parse, 'function');

const inherited = Object.create({ set safeParse(v) { log.push('proto setter'); } });
init(inherited, 4);
assert.strictEqual(Object.hasOwn(inherited, 'safeParse'), false);

for (let j = 0; j < 50; j++) {
  const sealed = Object.preventExtensions({ parse: 0 });
  assert.throws(() => init(sealed, j), TypeError);
  assert.strictEqual(typeof sealed.parse, 'function');
}

assert.deepStrictEqual(log, ['setter function', 'proxy parse', 'proxy safeParse', 'proxy tag', 'proto setter']);
console.log('ok');
