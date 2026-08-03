const assert = require("assert");
const os = require("node:os");
const util = require("util");
assert.strictEqual(typeof os.platform(), "string");
assert.strictEqual(typeof os.tmpdir(), "string");
assert.strictEqual(util.format("hello %s %d", "world", 42), "hello world 42");
assert.strictEqual(util.format("100%%"), "100%");
assert.strictEqual(util.types.isDate(new Date()), true);
