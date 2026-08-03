const assert = require("assert");
const fs = require("fs");
const os = require("os");
assert.strictEqual(fs.constants.F_OK, 0);
assert.strictEqual(fs.constants.R_OK > 0, true);
assert.strictEqual(typeof os.constants.signals.SIGTERM, "number");
assert.strictEqual(typeof os.constants.errno.ENOENT, "number");
