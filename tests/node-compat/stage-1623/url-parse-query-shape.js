const assert = require("node:assert");
const url = require("node:url");

const parsed = url.parse("/foo/bar?baz=quux#frag", true);
assert.deepStrictEqual(Object.keys(parsed).sort(), [
  "auth",
  "hash",
  "host",
  "hostname",
  "href",
  "path",
  "pathname",
  "port",
  "protocol",
  "query",
  "search",
  "slashes",
]);
assert.strictEqual(parsed.query.baz, "quux");
console.log("URL parse query shape passed");
