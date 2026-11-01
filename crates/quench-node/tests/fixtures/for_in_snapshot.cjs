function keys(object) { const result = []; for (const key in object) result.push(key); return result; }
function show(value) { console.log(JSON.stringify(value)); }
for (let round = 0; round < 200; round++) {
  const object = {z: 1, 10: 2, 2: 3, a: 4, [Symbol('hidden')]: 5};
  const expected = '2,10,z,a';
  if (keys(object).join() !== expected) throw new Error('order');
  for (const outer in object) {
    if (keys({z: 8, 10: 9, 2: 0, a: 1}).join() !== expected) throw new Error('nested');
    if (!Object.hasOwn(object, outer)) throw new Error('membership');
  }
}
show(keys({z: 1, 10: 2, 2: 3, a: 4}));
let reads = 0;
const accessor = {get a() { reads++; return 1; }, b: 2};
show([keys(accessor), reads]);
const prototype = {a: 1, inherited: 2};
const child = Object.create(prototype);
Object.defineProperty(child, 'a', {value: 3, enumerable: false, configurable: true});
child.b = 4;
show(keys(child));
prototype.later = 5;
show(keys(child));
delete child.a;
show(keys(child));
const mutable = {a: 1, b: 2, c: 3};
const visited = [];
for (const key in mutable) {
  visited.push(key);
  if (key === 'a') { delete mutable.b; }
}
show(visited);
show(keys(mutable));
mutable.b = 4;
show(keys(mutable));
const log = [];
const proxy = new Proxy({a: 1, b: 2}, {
  ownKeys(target) { log.push('keys'); return Reflect.ownKeys(target); },
  getPrototypeOf(target) { log.push('proto'); return Reflect.getPrototypeOf(target); },
  getOwnPropertyDescriptor(target, key) { log.push('desc:' + key); return Reflect.getOwnPropertyDescriptor(target, key); }
});
show(keys(proxy)); show(Array.from(new Set(log)).sort());
try { keys(new Proxy({}, {ownKeys() { throw new Error('keys failed'); }})); }
catch (error) { show([error.name, error.message]); }
for (let round = 0; round < 200; round++) {
  const object = {['dynamic-' + round]: round, stable: 1};
  const before = keys(object).join();
  for (const key in object) {
    if (typeof gc === 'function') gc();
    if (keys(object).join() !== before) throw new Error('collection');
  }
}
show('done');

// For-in must never consult or close the public array iterator.
const originalIterator = Array.prototype[Symbol.iterator];
let iteratorCalls = 0;
Array.prototype[Symbol.iterator] = function () {
  iteratorCalls++;
  throw new Error('for-in used array iterator');
};
let isolated = '';
try { for (const key in {a: 1, b: 2}) isolated += key; }
finally { Array.prototype[Symbol.iterator] = originalIterator; }
show([isolated, iteratorCalls]);
const captures = [];
for (let key in {a: 1, b: 2, c: 3}) {
  captures.push(() => key);
  if (key === 'b') continue;
}
show(captures.map(fn => fn()));
const controls = [];
outer: for (const key in {a: 1, b: 2, c: 3}) {
  try {
    for (const inner in {x: 1, y: 2}) {
      controls.push(key + inner);
      if (key === 'a') continue outer;
      if (key === 'b') break outer;
    }
  } finally { controls.push('finally:' + key); }
}
show(controls);
function* suspended() { for (const key in {a: 1, b: 2}) yield key; }
const generator = suspended();
show([generator.next(), generator.return('stop'), generator.next()]);
async function resumed() {
  const result = [];
  for (const key in {a: 1, b: 2}) { await Promise.resolve(); result.push(key); }
  return result;
}
resumed().then(show);
show([keys(null), keys(undefined), keys('az'), keys(123), keys(new Uint8Array([1, 2]))]);
const closed = [];
const iterable = {
  [Symbol.iterator]() {
    return {next() { return {value: 1, done: false}; }, return() { closed.push('return'); return {}; }};
  }
};
for (const value of iterable) { for (const key in {a: 1}) { break; } break; }
show(closed);
