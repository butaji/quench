const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.fileURLToPath("file:///foo/bar"), "/foo/bar");
assert.strictEqual(url.fileURLToPath("file:///foo%20bar"), "/foo bar");
assert.strictEqual(url.fileURLToPath("file:///foo%23bar"), "/foo#bar");
assert.strictEqual(url.pathToFileURL("/foo/bar").href, "file:///foo/bar");
assert.strictEqual(url.pathToFileURL("/foo bar").href, "file:///foo%20bar");
assert.strictEqual(url.pathToFileURL("/foo%bar").href, "file:///foo%25bar");
console.log("file path conversion matrix passed");
