const assert = require("node:assert");
const url = require("node:url");

const parsed = url.parse("mailto:foo@bar.com?subject=hello");
assert.strictEqual(parsed.protocol, "mailto:");
assert.strictEqual(parsed.auth, "foo");
assert.strictEqual(parsed.host, "bar.com");
assert.strictEqual(parsed.path, "?subject=hello");
console.log("mailto legacy URL parsing passed");
