const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("http://a/b/c/d;p?q", "g:h"), "g:h");
console.log("url resolve distinct scheme passed");
