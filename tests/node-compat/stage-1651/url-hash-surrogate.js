const assert = require("node:assert");
const { URL } = require("node:url");
const url = new URL("https://github.com/");
url.hash = "😀";
assert.strictEqual(url.href, "https://github.com/#%F0%9F%98%80");
console.log("URL hash surrogate passed");
