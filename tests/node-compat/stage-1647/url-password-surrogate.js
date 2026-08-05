const assert = require("node:assert");
const { URL } = require("node:url");
const url = new URL("https://github.com/");
url.password = "😀";
assert.strictEqual(url.password, "%F0%9F%98%80");
assert.strictEqual(url.href, "https://github.com/");
console.log("URL password surrogate passed");
