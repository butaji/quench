const assert = require("assert");
const cluster = require("cluster");

cluster.setupPrimary({ exec: "overridden" });
cluster.setupPrimary({ args: ["foo", "bar"] });
cluster.setupPrimary({ execArgv: ["baz", "bang"] });

assert.strictEqual(cluster.settings.exec, "overridden");
assert.deepStrictEqual(cluster.settings.args, ["foo", "bar"]);
assert.deepStrictEqual(cluster.settings.execArgv, ["baz", "bang"]);

console.log("cluster cumulative setup passed");
