const assert = require("node:assert");
const http = require("node:http");

const defaults = http.createServer();
assert.strictEqual(defaults.requestTimeout, 300000);
assert.strictEqual(defaults.headersTimeout, 60000);
assert.strictEqual(defaults.keepAliveTimeout, 5000);
assert.strictEqual(defaults.keepAliveTimeoutBuffer, 1000);
assert.strictEqual(defaults.connectionsCheckingInterval, 30000);
assert.strictEqual(defaults.timeout, 0);
assert.strictEqual(defaults.maxHeadersCount, null);
assert.strictEqual(defaults.maxRequestsPerSocket, 0);
assert.strictEqual(defaults.httpAllowHalfOpen, false);

const configured = http.createServer(
  {
    requestTimeout: 20000,
    keepAliveTimeout: 12,
    keepAliveTimeoutBuffer: 1500,
    connectionsCheckingInterval: 1000,
    highWaterMark: 4096,
  },
  () => {},
);
assert.strictEqual(configured.requestTimeout, 20000);
assert.strictEqual(configured.headersTimeout, 20000);
assert.strictEqual(configured.keepAliveTimeout, 12);
assert.strictEqual(configured.keepAliveTimeoutBuffer, 1500);
assert.strictEqual(configured.connectionsCheckingInterval, 1000);
assert.strictEqual(configured.highWaterMark, 4096);
assert.strictEqual(configured.setTimeout(250), configured);
assert.strictEqual(configured.timeout, 250);

assert.throws(() => http.createServer({ headersTimeout: "x" }), {
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => http.createServer({ requestTimeout: -1 }), {
  code: "ERR_OUT_OF_RANGE",
});
assert.throws(
  () => http.createServer({ headersTimeout: 10000, requestTimeout: 1000 }),
  { code: "ERR_OUT_OF_RANGE" },
);

console.log("http server timeout options passed");
