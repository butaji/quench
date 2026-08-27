'use strict';

const assert = require('assert');
const nodeTest = require('node:test');

assert.strictEqual(typeof nodeTest, 'function');
assert.strictEqual(typeof nodeTest.mock, 'object');
assert.strictEqual(typeof nodeTest.mock.fn, 'function');

nodeTest('context exposes mock', (t) => {
  assert.strictEqual(typeof t.mock, 'object');
  assert.strictEqual(typeof t.mock.fn, 'function');
  const direct = () => 7;
  assert.strictEqual(direct(), 7);
  const wrapped = t.mock.fn(direct);
  assert.strictEqual(typeof wrapped, 'function');
  assert.strictEqual(wrapped(), 7);
  const add = t.mock.fn((a, b) => a + b);
  assert.strictEqual(add(3, 4), 7);
  assert.strictEqual(add.mock.calls.length, 1);
  assert.deepStrictEqual(add.mock.calls[0].arguments, [3, 4]);
  const body = t.mock.fn(function (a, b) {
    return a + b;
  });
  assert.strictEqual(body(3, 4), 7);
});
