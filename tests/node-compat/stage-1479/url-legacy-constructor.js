const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(typeof url.Url, "function");
console.log("url legacy constructor passed");
