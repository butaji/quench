const assert = require('assert');
const { test, getTestContext } = require('node:test');

test('parent context is active in test-level hooks', (t) => {
  const parent = t.name;
  t.beforeEach(() => assert.strictEqual(getTestContext().name, parent));
  t.afterEach(() => assert.strictEqual(getTestContext().name, parent));
  t.test('child', () => assert.strictEqual(getTestContext().name, 'child'));
});
