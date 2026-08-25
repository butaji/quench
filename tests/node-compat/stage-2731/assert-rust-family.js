const assert = require('assert');
assert.ok(assert.AssertionError.prototype instanceof Error);

assert.deepStrictEqual({ value: [1, 2], nested: { ok: true } }, {
  value: [1, 2], nested: { ok: true },
});
assert.notDeepStrictEqual([1, 2], [1, 3]);
assert.deepStrictEqual(new Uint8Array([1, 2]), new Uint8Array([1, 2]));
assert.throws(() => assert.deepStrictEqual([1], [2]), {
  code: 'ERR_ASSERTION',
  operator: 'deepStrictEqual',
});
