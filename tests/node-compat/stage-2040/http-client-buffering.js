const assert = require("assert");
const http = require("http");

const request = http.request({ method: "POST" });
assert.strictEqual(request.cork(), request);
assert.strictEqual(request.cork(), request);
assert.strictEqual(request.uncork(), request);
assert.strictEqual(request.uncork(), request);
assert.strictEqual(request.uncork(), request);
request.destroy();

console.log("http client buffering passed");
