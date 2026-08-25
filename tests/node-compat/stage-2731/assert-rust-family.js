const assert = require('assert');
assert.ok(assert.AssertionError.prototype instanceof Error);

assert.deepStrictEqual({ value: [1, 2], nested: { ok: true } }, {
  value: [1, 2], nested: { ok: true },
});
assert.notDeepStrictEqual([1, 2], [1, 3]);
assert.deepStrictEqual(new Uint8Array([1, 2]), new Uint8Array([1, 2]));
assert.partialDeepStrictEqual({ value: [1, 2], extra: true }, { value: [1] });
assert.partialDeepStrictEqual([1, 2, 3], [1, 2]);
assert.throws(() => assert.partialDeepStrictEqual({ value: 1 }, { value: 2 }), {
  code: 'ERR_ASSERTION',
  operator: 'partialDeepStrictEqual',
});
assert.throws(() => assert.deepStrictEqual([1], [2]), {
  code: 'ERR_ASSERTION',
  operator: 'deepStrictEqual',
});
