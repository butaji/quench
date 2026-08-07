const assert = require("assert");
const http = require("http");

const request = http.request({ method: "GET" });
assert.strictEqual(request.setNoDelay(), request);
assert.strictEqual(request.setNoDelay(false), request);
assert.strictEqual(request.setSocketKeepAlive(), request);
assert.strictEqual(request.setSocketKeepAlive(true, 100), request);
assert.strictEqual(request.setSocketTimeout(1000), request);
request.destroy();

console.log("http client socket controls passed");
