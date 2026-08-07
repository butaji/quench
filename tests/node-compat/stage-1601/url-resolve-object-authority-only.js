const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["http://example", "foo", "http://example/foo"],
  ["http://example", "/foo", "http://example/foo"],
  ["http://example", "?query", "http://example/?query"],
  ["http://example", "#fragment", "http://example/#fragment"],
  ["http://example/", "", "http://example/"],
  ["https://example.com", "foo", "https://example.com/foo"],
  ["http://example/a", "//other", "http://other/"],
  ["http://example/a", "//other/path", "http://other/path"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("authority-only matrix passed");
