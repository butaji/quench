const assert = require("assert");
const cluster = require("cluster");

assert.strictEqual(cluster.schedulingPolicy, cluster.SCHED_RR);
assert.strictEqual(cluster.schedulingPolicy, 2);

console.log("cluster scheduling policy passed");
