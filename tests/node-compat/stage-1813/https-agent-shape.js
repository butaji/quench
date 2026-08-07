const assert = require("assert");
const https = require("https");
const http = require("http");

assert.strictEqual(typeof https.Agent, "function");
assert.ok(https.globalAgent instanceof https.Agent);
assert.ok(https.globalAgent instanceof http.Agent);
assert.strictEqual(https.globalAgent.defaultPort, 443);
assert.strictEqual(https.globalAgent.protocol, "https:");
assert.strictEqual(https.globalAgent.keepAlive, true);
assert.strictEqual(https.globalAgent.maxSockets, Infinity);
assert.strictEqual(https.globalAgent.maxFreeSockets, 256);
assert.strictEqual(https.globalAgent.scheduling, "lifo");

assert.throws(() => https.request("https://example.test"), {
  code: "ERR_TLS_NOT_SUPPORTED",
});

console.log("https Agent shape passed");
