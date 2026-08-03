const assert = require("assert");
const cluster = require("cluster");

cluster.setupPrimary();

assert.deepStrictEqual(cluster.settings.args, process.argv.slice(2));
assert.strictEqual(cluster.settings.exec, process.argv[1]);
assert.deepStrictEqual(cluster.settings.execArgv, process.execArgv || []);
assert.strictEqual(cluster.settings.silent, false);

console.log("cluster setup defaults passed");
