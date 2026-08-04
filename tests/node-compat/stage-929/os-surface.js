const assert = require("assert");
const os = require("os");

assert.strictEqual(typeof os.platform(), "string");
assert.strictEqual(typeof os.arch(), "string");
assert.strictEqual(typeof os.type(), "string");
assert.strictEqual(typeof os.release(), "string");
assert.strictEqual(typeof os.hostname(), "string");
assert.strictEqual(typeof os.tmpdir(), "string");
assert.strictEqual(typeof os.homedir(), "string");
assert.strictEqual(typeof os.endianness(), "string");
assert.strictEqual(typeof os.uptime(), "number");
assert.strictEqual(typeof os.totalmem(), "number");
assert.strictEqual(typeof os.freemem(), "number");
