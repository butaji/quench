const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("f:/a", ".//g"), "f://g");
console.log("opaque double-slash resolution passed");
