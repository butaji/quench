const assert = require("node:assert");
const { URL } = require("node:url");
const url = new URL("https://example.com/?q=old");
const params = url.searchParams;
url.href = "http://example.com/?q=new";
assert.strictEqual(params.get("q"), "new");
console.log("URL href searchParams sync passed");
