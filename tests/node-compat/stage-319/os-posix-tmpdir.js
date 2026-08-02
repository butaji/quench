const assert = require("assert");
const os = require("os");

process.env.TMPDIR = "/tmpdir\\";
assert.strictEqual(os.tmpdir(), "/tmpdir\\");
process.env.TMPDIR = "/";
assert.strictEqual(os.tmpdir(), "/");
