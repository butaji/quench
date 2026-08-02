const assert = require("assert");
const cluster = require("cluster");

const seen = [];
cluster.on("setup", () => seen.push({ ...cluster.settings }));
cluster.setupPrimary({ exec: "node-next" });
setImmediate(() => {
  assert.deepStrictEqual(seen, [{ exec: "node-next" }]);
});
