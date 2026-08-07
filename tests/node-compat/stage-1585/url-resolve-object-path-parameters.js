const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["http://a/b/c/d;p=1/2?q", "g", "http://a/b/c/d;p=1/2/g"],
  ["http://a/b/c/d;p=1/2?q", "./g", "http://a/b/c/d;p=1/2/g"],
  ["http://a/b/c/d;p=1/2?q", "g/", "http://a/b/c/d;p=1/2/g/"],
  ["http://a/b/c/d;p=1/2?q", "g?y", "http://a/b/c/d;p=1/2/g?y"],
  ["http://a/b/c/d;p=1/2?q", ";x", "http://a/b/c/d;p=1/2/;x"],
  ["http://a/b/c/d;p=1/2?q", "g;x", "http://a/b/c/d;p=1/2/g;x"],
  ["http://a/b/c/d;p=1/2?q", "g;x=1/./y", "http://a/b/c/d;p=1/2/g;x=1/y"],
  ["http://a/b/c/d;p=1/2?q", "g;x=1/../y", "http://a/b/c/d;p=1/2/y"],
  ["http://a/b/c/d;p=1/2?q", "../g", "http://a/b/c/g"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("path parameter matrix passed");
