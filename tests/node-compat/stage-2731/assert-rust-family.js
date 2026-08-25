const assert = require('assert');

assert.deepStrictEqual({ value: [1, 2], nested: { ok: true } }, {
  value: [1, 2], nested: { ok: true },
});
assert.notDeepStrictEqual([1, 2], [1, 3]);
assert.deepStrictEqual(new Uint8Array([1, 2]), new Uint8Array([1, 2]));
