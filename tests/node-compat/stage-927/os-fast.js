const assert = require("assert");
const os = require("os");

assert.strictEqual(typeof os.platform(), "string");
assert.strictEqual(typeof os.arch(), "string");
assert.strictEqual(typeof os.type(), "string");
assert.strictEqual(typeof os.release(), "string");
assert.strictEqual(typeof os.hostname(), "string");
assert.ok(Array.isArray(os.loadavg()));
assert.ok(Array.isArray(os.cpus()));
assert.ok(os.cpus().length > 0);
