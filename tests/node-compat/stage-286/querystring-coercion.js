const assert = require("assert");
const querystring = require("querystring");

assert.strictEqual(
  querystring.stringify({ nan: NaN, inf: Infinity }),
  "nan=&inf=",
);
assert.deepStrictEqual(querystring.parse("a", []), { a: "" });
assert.deepStrictEqual(querystring.parse("a", null, []), { "": "a" });
assert.strictEqual(
  Object.keys(querystring.parse("a=1&b=1", null, null, { maxKeys: 1 })).length,
  1,
);
