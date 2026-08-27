const assert = require('assert');
const { test } = require('node:test');

test('TestContext.assert has the filtered assertion surface', (t) => {
  for (const key of ['ok', 'strictEqual', 'rejects', 'doesNotReject', 'snapshot', 'fileSnapshot']) {
    assert.strictEqual(typeof t.assert[key], 'function');
  }
  for (const key of ['Assert', 'AssertionError', 'strict']) {
    assert.strictEqual(t.assert[key], undefined);
  }
});
