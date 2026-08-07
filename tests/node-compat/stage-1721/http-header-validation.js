const assert = require("node:assert");
const http = require("node:http");

assert.strictEqual(http.validateHeaderName("x-test"), undefined);
assert.throws(() => http.validateHeaderName("bad name"), {
  code: "ERR_INVALID_HTTP_TOKEN",
});
assert.strictEqual(http.validateHeaderValue("x-test", "ok"), undefined);
assert.throws(() => http.validateHeaderValue("x-test", "bad\nvalue"), {
  code: "ERR_INVALID_CHAR",
});

const internal = require("_http_common");
assert.strictEqual(internal._checkIsHttpToken("x-test"), true);
assert.strictEqual(internal._checkIsHttpToken("bad name"), false);
assert.strictEqual(internal._checkInvalidHeaderChar("bad\nvalue"), true);
console.log("http header validation passed");
