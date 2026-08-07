const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["fred:///s//a/b/c", "g", "fred:///s//a/b/g"],
  ["fred:///s//a/b/c", "./g", "fred:///s//a/b/g"],
  ["fred:///s//a/b/c", "g/", "fred:///s//a/b/g/"],
  ["fred:///s//a/b/c", "/g", "fred:///g"],
  ["fred:///s//a/b/c", "//g", "fred://g"],
  ["http:///s//a/b/c", "g", "http:///s//a/b/g"],
];
for (const [from, to, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), to),
    url.parse(expected),
    `${from} + ${to}`,
  );
}
console.log("parsed triple-slash matrix passed");
