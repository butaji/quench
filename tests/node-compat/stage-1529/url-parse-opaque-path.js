const assert = require("node:assert");
const url = require("node:url");

const parsed = url.parse("dash-test:foo/bar");
assert.strictEqual(parsed.protocol, "dash-test:");
assert.strictEqual(parsed.host, "foo");
assert.strictEqual(parsed.pathname, "/bar");
assert.strictEqual(parsed.path, "/bar");
console.log("opaque scheme URL paths passed");
