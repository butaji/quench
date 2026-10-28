'use strict';
const object = {};
const keys = [];
const expected = new Map();
function getterFor(index) {
  const captured = { value: index };
  return function () { return captured.value; };
}
function verify() {
  gc();
  const actual = Reflect.ownKeys(object);
  if (actual.length !== expected.size) throw new Error('key count');
  for (const key of actual) {
    if (object[key] !== expected.get(key)) throw new Error('getter lifetime');
  }
}
// Cross the 64-slot boundary and verify after each transition, including
// accessor replacement, conversion to data, deletion and re-addition.
for (let index = 0; index < 66; index++) {
  const key = Symbol(String(index));
  keys.push(key);
  Object.defineProperty(object, key, { get: getterFor(index), configurable: true });
  expected.set(key, index);
  verify();
  Object.defineProperty(object, key, { get: getterFor(index + 100) });
  expected.set(key, index + 100);
  verify();
  Object.defineProperty(object, key, { value: index + 200, writable: true });
  expected.set(key, index + 200);
  verify();
  if (index % 2 === 0) {
    delete object[key];
    expected.delete(key);
    verify();
    Object.defineProperty(object, key, { get: getterFor(index + 300), configurable: true });
    expected.set(key, index + 300);
    verify();
  }
}
console.log('shape lifecycle ok: ' + Reflect.ownKeys(object).length);
