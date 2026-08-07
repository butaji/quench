const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["http://a/b/c/d;p?q=1/2", "g", "http://a/b/c/g"],
  ["http://a/b/c/d;p?q=1/2", "./g", "http://a/b/c/g"],
  ["http://a/b/c/d;p?q=1/2", "g/", "http://a/b/c/g/"],
  ["http://a/b/c/d;p?q=1/2", "/g", "http://a/g"],
  ["http://a/b/c/d;p?q=1/2", "//g", "http://g/"],
  ["http://a/b/c/d;p?q=1/2", "?y", "http://a/b/c/d;p?y"],
  ["http://a/b/c/d;p?q=1/2", "g?y", "http://a/b/c/g?y"],
  ["http://a/b/c/d;p?q=1/2", "g?y/./x", "http://a/b/c/g?y/./x"],
  ["http://a/b/c/d;p?q=1/2", "g#s/../x", "http://a/b/c/g#s/../x"],
  ["http://a/b/c/d;p?q=1/2", "../g", "http://a/b/g"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("query base matrix passed");
