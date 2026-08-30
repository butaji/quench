const assert = require("assert");
const cluster = require("cluster");

if (cluster.isPrimary) {
  const worker = cluster.fork({ __quench_cluster_env: "custom" });
  worker.once("message", (message) => {
    assert.strictEqual(message.value, "custom");
    process.exit(0);
  });
} else {
  cluster.worker.send({ value: process.env.__quench_cluster_env });
}
