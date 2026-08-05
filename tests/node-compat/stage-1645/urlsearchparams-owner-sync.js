const assert = require("node:assert");
const { URL } = require("node:url");
const url = new URL("https://example.org?foo=~bar");
assert.strictEqual(url.searchParams.__nodeURLOwner, url);
url.searchParams.sort();
assert.strictEqual(url.search, "?foo=%7Ebar");
console.log("URLSearchParams owner sync passed");
