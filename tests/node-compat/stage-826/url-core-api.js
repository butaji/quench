"use strict";

const assert = require("assert");
const urlApi = require("node:url");

for (
  const name of [
    "URL",
    "URLSearchParams",
    "fileURLToPath",
    "pathToFileURL",
    "format",
    "parse",
    "resolve",
  ]
) {
  assert.strictEqual(typeof urlApi[name], "function");
}
const parsed = new urlApi.URL("https://example.com/path?value=1");
assert.strictEqual(parsed.hostname, "example.com");
assert.strictEqual(parsed.searchParams.get("value"), "1");

console.log("url core api passed");
