const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("http:///s//a/b/c", "//g"), "http://g/");
console.log("network-path URL trailing slash passed");
