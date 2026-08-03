const assert = require("assert");
const os = require("os");
assert.strictEqual(os.cpus().length > 0, true);
assert.strictEqual(typeof os.cpus()[0].model, "string");
