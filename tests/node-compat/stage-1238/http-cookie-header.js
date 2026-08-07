const assert = require("assert");
const http = require("http");

const request = http.get(
  {
    port: 0,
    headers: { Cookie: ["foo=bar", "bar=baz", "baz=quux"] },
  },
  () => {},
);
assert.strictEqual(request._header, "Cookie: foo=bar; bar=baz; baz=quux");
