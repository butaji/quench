const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("http:///s//a/b/c", "g"), "http:///s//a/b/g");
console.log("empty-authority URL resolution passed");
