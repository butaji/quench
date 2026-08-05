const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("http://a/b/c/d;p?q", "//g"), "http://g/");
console.log("url resolve network path passed");
