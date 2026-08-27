'use strict';

const assert = require('assert');
const nodeTest = require('node:test');

assert.strictEqual(typeof nodeTest, 'function');
assert.strictEqual(typeof nodeTest.mock, 'object');
assert.strictEqual(typeof nodeTest.mock.fn, 'function');

nodeTest('context exposes mock', (t) => {
  assert.strictEqual(typeof t.mock, 'object');
  assert.strictEqual(typeof t.mock.fn, 'function');
});
