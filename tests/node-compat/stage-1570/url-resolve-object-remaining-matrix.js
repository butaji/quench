const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["http://example.com/b//c//d;p?q#blarg", "https:/a/b/c/d", "https://a/b/c/d"],
  [
    "http://example.com/b//c//d;p?q#blarg",
    "http:#hash2",
    "http://example.com/b//c//d;p?q#hash2",
  ],
  [
    "http://example.com/b//c//d;p?q#blarg",
    "http:/p/a/t/h?s#hash2",
    "http://example.com/p/a/t/h?s#hash2",
  ],
  ["/foo/bar/baz", "/../etc/passwd", "/etc/passwd"],
  ["http://localhost", "file:///Users/foo", "file:///Users/foo"],
  ["http://localhost", "file://foo/Users", "file://foo/Users"],
  [
    "https://registry.npmjs.org",
    "@foo/bar",
    "https://registry.npmjs.org/@foo/bar",
  ],
];
for (const [from, to, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), to),
    url.parse(expected),
    `${from} + ${to}`,
  );
}
console.log("remaining parsed URL matrix passed");
