const assert = require("assert");
const helper = require("../common/child_process");

const result = helper.spawnSyncAndAssert(
  "quench-node",
  ["message.mjs"],
  {},
  {
    status: 0,
    signal: null,
    stderr: (value) => {
      assert.match(value, /message\.mjs was not initialized/);
      return true;
    },
  },
);
assert.strictEqual(result.status, 0);
