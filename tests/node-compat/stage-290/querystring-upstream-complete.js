const assert = require("assert");
const querystring = require("querystring");

assert.deepStrictEqual(querystring.parse("foo&bar", "&", "&"), {
  foo: "",
  bar: "",
});
assert.strictEqual(
  querystring.stringify({ foo: "bar", list: ["a", "b"] }),
  "foo=bar&list=a&list=b",
);

const previous = querystring.unescape;
querystring.unescape = (value) => value.replace(/o/g, "_");
assert.deepStrictEqual(querystring.parse("foo=bor"), { f__: "b_r" });
querystring.unescape = previous;
