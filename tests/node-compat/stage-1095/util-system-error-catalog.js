const assert = require("node:assert");
const util = require("node:util");

const errorMap = util.getSystemErrorMap();
assert.deepStrictEqual(errorMap.get(-32), ["EPIPE", "EPIPE"]);
assert.deepStrictEqual(errorMap.get(-105), ["ENOBUFS", "ENOBUFS"]);
assert.strictEqual(util.getSystemErrorName(-32), "EPIPE");
assert.strictEqual(util.getSystemErrorName(-105), "ENOBUFS");
