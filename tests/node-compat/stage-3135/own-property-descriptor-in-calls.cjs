// Object.getOwnPropertyDescriptor called from nested calls inside a loop: the descriptor
// it returns, and what guest code it reaches (key coercion, proxy traps) sees of the stack.
const assert = require('assert');

function names(stack) {
  return stack.split('\n').slice(1).map((line) => line.trim().split(' ')[1]);
}

function readDescriptor(target, key) {
  return Object.getOwnPropertyDescriptor(target, key);
}

function viaHelper(target, key) {
  return readDescriptor(target, key);
}

let getterRuns = 0;
const plain = {
  value: 1,
  get accessor() { getterRuns++; return 2; },
};
Object.defineProperty(plain, 'hidden', { value: 3, enumerable: false });

for (let i = 0; i < 200; i++) {
  const data = viaHelper(plain, 'value');
  assert.deepStrictEqual(data, { value: 1, writable: true, enumerable: true, configurable: true });
  const accessor = viaHelper(plain, 'accessor');
  assert.strictEqual(typeof accessor.get, 'function');
  assert.strictEqual(accessor.set, undefined);
  assert.deepStrictEqual(viaHelper(plain, 'hidden'),
    { value: 3, writable: false, enumerable: false, configurable: false });
  assert.strictEqual(viaHelper(plain, 'missing'), undefined);
}
assert.strictEqual(getterRuns, 0);

// A trap sees its callers on the stack, and its error leaves through them.
let trapStack;
const traced = new Proxy({}, {
  getOwnPropertyDescriptor(target, key) {
    trapStack = names(new Error().stack);
    if (key === 'fail') throw new TypeError('trap failed');
    return { value: key, writable: true, enumerable: true, configurable: true };
  },
});
for (let i = 0; i < 50; i++) {
  assert.strictEqual(viaHelper(traced, 'k').value, 'k');
  assert.ok(trapStack.includes('readDescriptor') && trapStack.includes('viaHelper'), String(trapStack));
  let thrown;
  try { viaHelper(traced, 'fail'); } catch (error) { thrown = error; }
  assert.ok(thrown instanceof TypeError);
  const thrownNames = names(thrown.stack);
  assert.ok(thrownNames.includes('readDescriptor') && thrownNames.includes('viaHelper'), thrown.stack);
}

// Key coercion runs guest code from the same calls.
let keyStack;
const key = { toString() { keyStack = names(new Error().stack); return 'value'; } };
for (let i = 0; i < 50; i++) {
  assert.strictEqual(viaHelper(plain, key).value, 1);
  assert.ok(keyStack.includes('readDescriptor') && keyStack.includes('viaHelper'), String(keyStack));
}

// A mapped arguments object reads the parameter it aliases.
function mapped(a) {
  a = 7;
  const first = readDescriptor(arguments, 0).value;
  arguments[0] = 9;
  return [first, a, readDescriptor(arguments, 0).value];
}
for (let i = 0; i < 50; i++) {
  assert.deepStrictEqual(mapped(1), [7, 9, 9]);
}

console.log('own-property-descriptor-in-calls ok');
