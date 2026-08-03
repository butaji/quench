const assert = require("assert");
const cluster = require("cluster");

assert.strictEqual(Array.isArray(cluster.workers), false);
assert.deepStrictEqual(Object.keys(cluster.workers), []);

if (cluster.isPrimary) {
  const worker = cluster.fork();
  worker.once("online", () => {
    assert.strictEqual(cluster.workers[worker.id], worker);
    worker.kill();
  });
}

console.log("cluster workers object passed");
