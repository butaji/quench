const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["/foo bar", "file:///foo%20bar"],
  ["/foo?bar", "file:///foo%3Fbar"],
  ["/foo#bar", "file:///foo%23bar"],
  ["/foo&bar", "file:///foo&bar"],
  ["/foo=bar", "file:///foo=bar"],
  ["/foo:bar", "file:///foo:bar"],
  ["/foo;bar", "file:///foo;bar"],
  ["/foo%bar", "file:///foo%25bar"],
  ["/foo\\bar", "file:///foo%5Cbar"],
];
for (const [path, expected] of cases) {
  assert.strictEqual(url.pathToFileURL(path).href, expected, path);
}
console.log("reserved file URL matrix passed");
