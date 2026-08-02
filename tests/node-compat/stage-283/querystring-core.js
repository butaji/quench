const assert = require("assert");
const querystring = require("querystring");

assert.deepStrictEqual(querystring.parse("foo=bar&foo=quux"), {
  foo: ["bar", "quux"],
});
assert.strictEqual(Object.getPrototypeOf(querystring.parse("a=b")), null);
assert.deepStrictEqual(querystring.parse("foo+bar=baz+quux"), {
  "foo bar": "baz quux",
});
assert.deepStrictEqual(querystring.parse("foo=%zx&empty"), {
  foo: "%zx",
  empty: "",
});
assert.strictEqual(
  querystring.stringify({ a: "!-._~'()*", empty: null }),
  "a=!-._~'()*&empty=",
);
assert.deepStrictEqual(querystring.parse(null), {});
