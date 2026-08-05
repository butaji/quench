const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("/x/y?q", "http://ex?p"), "http://ex/x/y?q");
console.log("query-base URL resolution passed");
