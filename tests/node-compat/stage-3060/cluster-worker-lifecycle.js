const assert = require("assert");
const cluster = require("cluster");

if (cluster.isPrimary) {
  const worker = cluster.fork();
  worker.on("exit", (code) => {
    assert.strictEqual(code, 2);
    process.exit(0);
  });
  assert.strictEqual(worker.send("message"), true);
} else {
  assert.strictEqual(cluster.isWorker, true);
  assert.strictEqual(cluster.worker.id > 0, true);
  process.on("message", () => process.exit(2));
}
