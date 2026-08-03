const assert = require("assert");
const cluster = require("cluster");

const seen = [];
cluster.on("setup", () => seen.push({ ...cluster.settings }));
cluster.setupPrimary({ exec: "node-next" });
setImmediate(() => {
  assert.strictEqual(seen.length, 1);
  assert.strictEqual(seen[0].exec, "node-next");
  assert.deepStrictEqual(seen[0].args, process.argv.slice(2));
  assert.deepStrictEqual(seen[0].execArgv, process.execArgv || []);
  assert.strictEqual(seen[0].silent, false);
});
