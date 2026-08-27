const assert = require('assert');
const { test, getTestContext } = require('node:test');
test('async context survives immediate', async () => {
  const expected = getTestContext().name;
  const got = await new Promise((resolve) => setImmediate(() => resolve(getTestContext())));
  assert.strictEqual(got.name, expected);
});
