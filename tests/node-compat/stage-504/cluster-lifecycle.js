const assert = require("assert");
const cluster = require("cluster");

assert.strictEqual(cluster.isPrimary, true);
const worker = cluster.fork();
worker.on("online", () => {
  assert.strictEqual(typeof worker.disconnect, "function");
  worker.disconnect();
});
