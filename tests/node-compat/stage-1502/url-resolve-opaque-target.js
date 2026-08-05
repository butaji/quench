const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("foo:.", "foo:a"), "foo:a");
assert.strictEqual(url.resolve("foo:a", "foo:."), "foo:");
console.log("url resolve opaque target passed");
