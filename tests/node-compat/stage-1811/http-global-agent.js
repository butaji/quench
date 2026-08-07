const assert = require("assert");
const http = require("http");

assert.ok(http.globalAgent instanceof http.Agent);
assert.strictEqual(http.globalAgent.keepAlive, true);
assert.strictEqual(http.globalAgent.maxSockets, Infinity);
assert.strictEqual(http.globalAgent.maxFreeSockets, 256);
assert.strictEqual(http.globalAgent.maxTotalSockets, Infinity);
assert.strictEqual(http.globalAgent.scheduling, "lifo");
assert.strictEqual(typeof http.globalAgent.getName, "function");
assert.strictEqual(typeof http.globalAgent.getCurrentStatus, "function");

const custom = new http.Agent({
  keepAlive: false,
  maxSockets: 3,
  maxFreeSockets: 2,
  maxTotalSockets: 4,
  scheduling: "fifo",
});
assert.strictEqual(custom.keepAlive, false);
assert.strictEqual(custom.maxSockets, 3);
assert.strictEqual(custom.maxFreeSockets, 2);
assert.strictEqual(custom.maxTotalSockets, 4);
assert.strictEqual(custom.scheduling, "fifo");

console.log("http globalAgent passed");
