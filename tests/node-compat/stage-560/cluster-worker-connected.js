const assert = require("assert");
const cluster = require("cluster");

if (cluster.isPrimary) {
  const worker = cluster.fork();
  worker.once("online", () => {
    assert.strictEqual(worker.isConnected(), true);
    worker.kill();
  });
} else {
  assert.strictEqual(cluster.worker.isConnected(), true);
}

console.log("cluster worker connection state passed");
