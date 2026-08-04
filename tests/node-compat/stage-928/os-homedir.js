const assert = require("assert");
const os = require("os");

assert.strictEqual(typeof os.homedir(), "string");
assert.ok(os.homedir().length > 0);
