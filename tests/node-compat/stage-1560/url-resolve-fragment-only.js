const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("#hash2", "#hash1"), "/#hash1");
console.log("fragment-only resolution passed");
