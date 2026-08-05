const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("foo/bar", "../../../baz"), "../../baz");
console.log("url resolve relative parent passed");
