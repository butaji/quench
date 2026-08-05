const assert = require("node:assert");
const url = require("node:url");

const base = "http://a/b/c/d;p?q";
assert.strictEqual(url.resolve(base, ""), base);
console.log("url resolve empty target passed");
