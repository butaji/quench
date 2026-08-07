const assert = require("node:assert");
const querystring = require("querystring");
const encoded = querystring.stringify({
  a: "hello world",
  tag: ["one", "two"],
});
assert.strictEqual(encoded, "a=hello%20world&tag=one&tag=two");
assert.deepStrictEqual(querystring.parse(encoded), {
  a: "hello world",
  tag: ["one", "two"],
});
