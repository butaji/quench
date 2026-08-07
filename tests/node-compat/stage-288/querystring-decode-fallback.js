const assert = require("assert");
const querystring = require("querystring");

assert.deepStrictEqual(
  querystring.parse("a=a", null, null, {
    decodeURIComponent: () => {
      throw new Error("fallback");
    },
  }),
  { a: "a" },
);

const previous = querystring.unescape;
querystring.unescape = (value) => value.replace(/o/g, "_");
assert.deepStrictEqual(querystring.parse("foo=bor"), {
  f__: "b_r",
});
querystring.unescape = previous;
