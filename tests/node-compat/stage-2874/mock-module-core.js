const assert = require('assert');
const { test } = require('node:test');

test('mock.module replaces a core module with named exports', (t) => {
  const original = require('readline');
  assert.strictEqual(typeof original.createInterface, 'function');
  t.mock.module('readline', { namedExports: { fn() { return 42; } } });
  const mocked = require('readline');
  assert.strictEqual(mocked.fn(), 42);
  assert.strictEqual(mocked.createInterface, undefined);
  t.mock.reset();
  assert.strictEqual(require('readline'), original);
});
