const assert = require("assert");
const cluster = require("cluster");

if (cluster.isPrimary) {
  const worker = cluster.fork();
  worker.once("online", () => {
    assert.strictEqual(worker.isDead(), false);
    assert.strictEqual(typeof worker.destroy, "function");
    worker.destroy();
  });
} else {
  assert.strictEqual(cluster.worker.isDead(), false);
}

console.log("cluster worker state passed");
