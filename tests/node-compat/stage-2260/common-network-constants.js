const assert = require("assert");
const common = require("../../node/test/common");

assert.strictEqual(common.localhostIPv4, "127.0.0.1");
assert.strictEqual(common.localhostIPv6, "::1");
assert.strictEqual(common.hasIPv6, true);
console.log("common network constants passed");
