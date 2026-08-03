const assert = require("assert");
const cluster = require("cluster");

assert.strictEqual(cluster.setupMaster, cluster.setupPrimary);
assert.strictEqual(cluster.isMaster, cluster.isPrimary);

console.log("cluster aliases passed");
