const assert = require("assert");
const os = require("os");

assert.ok(os.tmpdir().length > 0);
assert.ok(os.cpus().length > 0);
assert.ok(os.availableParallelism() > 0);
assert.ok(os.totalmem() > 0);
assert.strictEqual(os.userInfo().uid, os.userInfo({ encoding: "buffer" }).uid);
assert.strictEqual(os.devNull, "/dev/null");
