const assert = require("assert");
const util = require("util");

assert.strictEqual(util.isArray([]), true);
assert.deepStrictEqual(util._extend({ a: 1 }, { b: 2 }), { a: 1, b: 2 });
assert.strictEqual(util.toUSVString("bad\ud801"), "bad\ufffd");
assert.strictEqual(
  util.stripVTControlCharacters("\u001b[31mready\u001b[0m"),
  "ready",
);
assert.strictEqual(util.types.isNativeError(new Error()), true);
