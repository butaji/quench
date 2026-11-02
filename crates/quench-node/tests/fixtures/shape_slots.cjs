'use strict';
const symbol = Symbol('slot');
function build() {
  const object = {};
  for (let i = 0; i < 300; i++) {
    object['k' + i] = i;
    if (i % 7 === 0) Object.defineProperty(object, 'k' + i, { enumerable: false });
    if (i % 11 === 0) Object.keys(object);
  }
  object[symbol] = 123;
  return object;
}
function snapshot(object) {
  return Reflect.ownKeys(object).map(key => {
    const d = Object.getOwnPropertyDescriptor(object, key);
    return [String(key), d.value, d.writable, d.enumerable, d.configurable,
      typeof d.get, typeof d.set, object[key]];
  });
}
const a = build(), b = build();
for (const object of [a, b]) {
  for (const index of [0, 63, 64, 127, 128, 255, 299]) {
    const key = 'k' + index;
    Object.defineProperty(object, key, { enumerable: true });
    Object.defineProperty(object, key, { get() { return index + 1000; }, configurable: true });
    Object.keys(object);
    Object.defineProperty(object, key, { value: index + 2000, writable: true });
  }
  let stored = 0;
  Object.defineProperty(object, 'k128', {
    get() { return stored; }, set(value) { stored = value; }, configurable: true
  });
  Object.defineProperty(object, 'k127', {
    set(value) { stored = value * 2; }, configurable: true
  });
  object.k128 = 12;
  object.k127 = 23;
  delete object.k65;
  object.k65 = 're-added';
  Object.defineProperty(object, symbol, { writable: false, enumerable: false });
}
gc();
console.log(JSON.stringify([snapshot(a), snapshot(b)]));
Object.freeze(a);
let error;
try { a.k64 = 0; } catch (e) { error = e.name; }
console.log(JSON.stringify([error, Object.isFrozen(a), snapshot(a)]));
