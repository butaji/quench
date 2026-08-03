const assert = require("assert");
const cluster = require("cluster");

if (cluster.isPrimary) {
  const worker = cluster.fork();
  worker.once("online", () => {
    assert.strictEqual(cluster.workers[worker.id], worker);
    worker.once("exit", () => {
      let attempts = 0;
      const check = () => {
        if (cluster.workers[worker.id] !== undefined && attempts++ < 20) {
          setImmediate(check);
          return;
        }
        assert.strictEqual(cluster.workers[worker.id], undefined);
        console.log("cluster workers cleanup passed");
      };
      check();
    });
    worker.kill();
  });
}
