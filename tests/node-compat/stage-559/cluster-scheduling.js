const assert = require("assert");
const cluster = require("cluster");

assert.strictEqual(cluster.SCHED_NONE, 1);
assert.strictEqual(cluster.SCHED_RR, 2);
assert.strictEqual(cluster.isPrimary, true);
assert.strictEqual(cluster.isWorker, false);
assert.strictEqual(cluster.isMaster, true);

console.log("cluster scheduling constants passed");
