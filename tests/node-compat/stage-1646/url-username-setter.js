const assert = require("node:assert");
const { URL } = require("node:url");
const url = new URL("https://github.com/");
url.username = "😀";
assert.strictEqual(url.href, "https://%F0%9F%98%80@github.com/");
console.log("URL username setter passed");
