const assert = require('assert');
const { test, getTestContext } = require('node:test');

test('getTestContext exposes the active named context', (t) => {
  assert.strictEqual(getTestContext().name, 'getTestContext exposes the active named context');
  assert.strictEqual(getTestContext().fullName, getTestContext().name);
  assert.strictEqual(typeof getTestContext().signal, 'object');
  assert.strictEqual(getTestContext().signal.aborted, false);
});
