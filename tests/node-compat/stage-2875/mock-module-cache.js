const assert = require('assert');
const { test } = require('node:test');

test('mock.module cache true preserves one replacement across aliases', (t) => {
  t.mock.module('readline', { namedExports: { fn() { return 42; } }, cache: true });
  const mocked = require('readline');
  assert.strictEqual(mocked, require('readline'));
  assert.strictEqual(mocked.fn(), 42);
  t.mock.reset();
  assert.strictEqual(typeof require('readline').createInterface, 'function');
});
