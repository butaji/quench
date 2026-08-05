const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("zz:abc", "/foo/../../../bar"), "zz:/bar");
console.log("url resolve opaque absolute path passed");
