const assert = require("assert");
const querystring = require("querystring");

assert.strictEqual(
  querystring.unescapeBuffer("%d3%f2Ug%1f6v%24%5e%98%cb")[2],
  0x55,
);
assert.strictEqual(querystring.unescapeBuffer("a+b", true).toString(), "a b");
assert.strictEqual(querystring.unescapeBuffer("a+b").toString(), "a+b");
assert.strictEqual(
  Object.keys(querystring.parse("&a", null, null, { maxKeys: 1 })).length,
  0,
);
assert.deepStrictEqual(
  querystring.parse("a=a&b=b", null, null, {
    decodeURIComponent: (value) => value + value,
  }),
  { aa: "aa", bb: "bb" },
);
