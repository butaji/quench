const assert = require("assert");
const cluster = require("cluster");

if (cluster.isPrimary) {
  const worker = cluster.fork();
  worker.once("online", () => {
    assert.strictEqual(worker.process.connected, true);
    assert.strictEqual(typeof worker.process.disconnect, "function");
    assert.strictEqual(typeof worker.process.send, "function");
    worker.disconnect();
  });
} else {
  assert.strictEqual(typeof cluster.worker.process.send, "function");
}

console.log("cluster process IPC passed");
