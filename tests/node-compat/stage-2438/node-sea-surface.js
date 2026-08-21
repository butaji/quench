const assert = require("assert");
const sea = require("node:sea");

assert.strictEqual(sea.isSea, false);
assert.strictEqual(typeof sea.getAsset, "function");
assert.throws(() => sea.getAsset("missing"), { code: "ERR_NOT_SUPPORTED" });
console.log("node:sea surface passed");
