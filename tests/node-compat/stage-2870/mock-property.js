const assert = require('assert');
const { test } = require('node:test');

test('mock.property uses one descriptor-backed value and access log', (t) => {
  const object = { value: 1 };
  const mock = t.mock.property(object, 'value', 2);

  assert.strictEqual(object.value, 2);
  object.value = 3;
  assert.strictEqual(object.value, 3);
  assert.deepStrictEqual(mock.mock.accesses.map((entry) => entry.type), ['get', 'set', 'get']);

  mock.mock.resetAccesses();
  mock.mock.mockImplementation(4);
  assert.strictEqual(object.value, 4);
  mock.mock.mockImplementationOnce(5);
  assert.strictEqual(object.value, 5);
  assert.strictEqual(object.value, 4);

  mock.mock.restore();
  assert.strictEqual(object.value, 1);
});
