const assert = require("node:assert");
const util = require("node:util");

const map = util.getSystemErrorMap();
assert.deepStrictEqual(map.get(-2), ["ENOENT", "ENOENT"]);
assert.deepStrictEqual(map.get(-17), ["EEXIST", "EEXIST"]);
assert.strictEqual(util._errnoException(-2, "open").errno, -2);
