const assert = require("node:assert");
const url = new URL("https://github.com/");
url.password = "😀";
assert.strictEqual(url.password, "%F0%9F%98%80");
assert.strictEqual(url.href, "https://:%F0%9F%98%80@github.com/");
console.log("Global URL password passed");
