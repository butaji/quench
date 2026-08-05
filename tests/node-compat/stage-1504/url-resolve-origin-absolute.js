const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("http://a/b/c/d;p?q", "/."), "http://a/");
console.log("url resolve origin absolute passed");
