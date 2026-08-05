const assert = require("node:assert");
const url = require("node:url");

const parsed = url.parse("/foo/bar?baz=quux#frag", true);
assert.deepStrictEqual(Object.keys(parsed.query), ["baz"]);
assert.strictEqual(parsed.query.baz, "quux");
assert.strictEqual(Object.getPrototypeOf(parsed.query), null);
console.log("url parse query object passed");
