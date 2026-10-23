import assert from 'node:assert/strict';

const sparse = [0, 1, true, null, {}, 'five'];
sparse[999999] = -6.6;

const calls = [];
assert.equal(sparse.every((value, index, receiver) => {
  calls.push([value, index, receiver]);
  return true;
}), true);
assert.deepEqual(calls.map(([, index]) => index), [0, 1, 2, 3, 4, 5, 999999]);
assert.ok(calls.every(([, , receiver]) => receiver === sparse));
assert.equal(sparse.some((value) => value === -6.6), true);
assert.equal(sparse.map((value) => value).length, 1000000);
assert.equal(6 in sparse, false);

// Indexed properties on the prototype are visible through holes.
const prototype = Object.create(Array.prototype);
prototype[100] = 'inherited';
const inherited = [];
Object.setPrototypeOf(inherited, prototype);
inherited.length = 1000;
const inheritedCalls = [];
inherited.every((value, index) => {
  inheritedCalls.push([value, index]);
  return true;
});
assert.deepEqual(inheritedCalls, [['inherited', 100]]);

// A callback can add a future indexed property; later HasProperty must observe it.
const mutated = [];
mutated.length = 8;
mutated[2] = 'start';
const mutatedCalls = [];
mutated.every((value, index) => {
  mutatedCalls.push(index);
  if (index === 2) mutated[6] = 'added';
  return true;
});
assert.deepEqual(mutatedCalls, [2, 6]);

// Proxy HasProperty traps keep their observable call order.
const proxyChecks = [];
const proxy = new Proxy([, 'value'], {
  has(target, key) {
    if (key === '0' || key === '1') proxyChecks.push(key);
    return Reflect.has(target, key);
  },
});
proxy.every(() => true);
assert.deepEqual(proxyChecks, ['0', '1']);
