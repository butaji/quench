"use strict";

const assert = require("assert");
const cluster = require("node:cluster");

for (const name of ["isPrimary", "isWorker", "worker", "workers", "settings"]) {
  assert.ok(name in cluster);
}
for (const name of ["fork", "setupPrimary", "disconnect", "schedulingPolicy"]) {
  assert.ok(name in cluster);
}
assert.strictEqual(typeof cluster.isPrimary, "boolean");
assert.strictEqual(typeof cluster.isWorker, "boolean");

console.log("cluster api passed");
