const assert = require("assert");
const os = require("os");

for (
  const value of [
    os.getPriority(),
    os.uptime(),
    os.userInfo().uid,
    os.userInfo().gid,
    +os.uptime,
    os.availableParallelism(),
    +os.availableParallelism,
    os.freemem(),
    +os.freemem,
  ]
) {
  assert.strictEqual(typeof value, "number");
  assert.ok(!Number.isNaN(value));
}
assert.ok(os.availableParallelism() > 0);
assert.ok(os.totalmem() > 0);
