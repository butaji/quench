const assert = require("assert");
const os = require("os");

process.env.TMPDIR = "/tmpdir/";
process.env.TMP = "/tmp";
process.env.TEMP = "/temp";
assert.strictEqual(os.tmpdir(), "/tmpdir");
process.env.TMPDIR = "";
assert.strictEqual(os.tmpdir(), "/tmp");
process.env.TMP = "";
assert.strictEqual(os.tmpdir(), "/temp");
