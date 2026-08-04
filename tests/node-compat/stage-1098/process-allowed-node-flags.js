const assert = require("node:assert");

const flags = process.allowedNodeEnvironmentFlags;
assert.strictEqual(flags.has("--perf_basic_prof"), true);
assert.strictEqual(flags.has("perf-basic-prof"), true);
assert.strictEqual(flags.has("--cheeseburgers"), false);
assert.strictEqual(Object.isFrozen(flags), true);
flags.add("custom");
Set.prototype.add.call(flags, "custom");
assert.strictEqual(flags.has("custom"), false);
flags.delete("-r");
Set.prototype.clear.call(flags);
assert.strictEqual(flags.has("-r"), true);
for (const flag of flags) {
  if (!/^--?[a-zA-Z0-9._-]+$/.test(flag)) throw new Error(flag);
}
