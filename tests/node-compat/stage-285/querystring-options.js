const assert = require("assert");
const querystring = require("querystring");

assert.strictEqual(
  querystring.stringify({ foo: 2n ** 64n }, null, null, {
    encodeURIComponent: (value) => value,
  }),
  "foo=18446744073709551616",
);

assert.throws(() => querystring.stringify({ foo: "\udc00" }), {
  code: "ERR_INVALID_URI",
  name: "URIError",
  message: "URI malformed",
});
