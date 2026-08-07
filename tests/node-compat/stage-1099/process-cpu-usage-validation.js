const assert = require("node:assert");

assert.deepStrictEqual(process.cpuUsage(), { user: 0, system: 0 });
assert.deepStrictEqual(process.cpuUsage({ user: 0, system: 0 }), {
  user: 0,
  system: 0,
});
assert.throws(() => process.cpuUsage(1), { code: "ERR_INVALID_ARG_TYPE" });
assert.throws(() => process.cpuUsage({ user: -1, system: 0 }), {
  code: "ERR_INVALID_ARG_VALUE",
});
