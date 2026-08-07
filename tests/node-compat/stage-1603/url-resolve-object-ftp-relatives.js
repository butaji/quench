const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["ftp://example.com/a/b/c", "g", "ftp://example.com/a/b/g"],
  ["ftp://example.com/a/b/c", "../g", "ftp://example.com/a/g"],
  ["ftp://example.com/a/b/c", "/g", "ftp://example.com/g"],
  ["ftp://example.com/a/b/c", "//other/g", "ftp://other/g"],
  [
    "ftp://example.com/a/b/c?query",
    "#fragment",
    "ftp://example.com/a/b/c?query#fragment",
  ],
  ["ftp://example.com/a/b/c", "g/", "ftp://example.com/a/b/g/"],
  ["ftp://example.com/a/b/c", "", "ftp://example.com/a/b/c"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("FTP relative matrix passed");
