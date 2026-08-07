const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["fred:///s//a/b/c", "//g/x", "fred://g/x"],
  ["fred:///s//a/b/c", "///g", "fred:///g"],
  ["http:///s//a/b/c", "//g", "http://g/"],
  ["http:///s//a/b/c", "//g/x", "http://g/x"],
  ["http:///s//a/b/c", "///g", "http:///g"],
  ["http:///s//a/b/c", "/g", "http:///g"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("triple-slash authority matrix passed");
