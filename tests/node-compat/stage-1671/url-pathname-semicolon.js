const assert = require("node:assert");

const url = new URL("http://user:pass@foo:21/bar;par?b#c");
assert.strictEqual(url.href, "http://user:pass@foo:21/bar;par?b#c");
console.log("URL pathname semicolon preservation passed");
