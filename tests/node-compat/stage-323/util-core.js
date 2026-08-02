const assert = require("assert");
const util = require("util");

assert.strictEqual(util.isArray([]), true);
assert.strictEqual(util.isArray({}), false);
assert.deepStrictEqual(util._extend({ a: 1 }, { b: 2 }), { a: 1, b: 2 });
assert.strictEqual(util.toUSVString("string\ud801"), "string\ufffd");
assert.strictEqual(
  util.stripVTControlCharacters("\u001b[31mfoo\u001b[39m"),
  "foo",
);
assert.strictEqual(
  util.stripVTControlCharacters("\u009b31mfoo\u009b39m"),
  "foo",
);
