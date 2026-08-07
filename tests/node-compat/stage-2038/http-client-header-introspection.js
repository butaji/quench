const assert = require("assert");
const http = require("http");

const request = http.request({ method: "POST", headers: { "X-Test": "one" } });
assert.strictEqual(request.getHeader("x-test"), "one");
assert.strictEqual(request.hasHeader("X-TEST"), true);
assert.deepStrictEqual(request.getHeaderNames(), ["x-test", "connection"]);
assert.deepStrictEqual(request.getHeaders(), {
  "x-test": "one",
  connection: "keep-alive"
});
request.setHeader("X-Other", "two");
assert.strictEqual(request.getHeader("x-other"), "two");
request.removeHeader("X-Test");
assert.strictEqual(request.hasHeader("x-test"), false);
request.destroy();

console.log("http client header introspection passed");
