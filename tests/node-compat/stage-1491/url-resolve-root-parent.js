const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("/foo", ".."), "/");
console.log("url resolve root parent passed");
