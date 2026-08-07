const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["http://a/b/c/d;p?q", "g:h", "g:h"],
  ["http://a/b/c/d;p?q", "g", "http://a/b/c/g"],
  ["http://a/b/c/d;p?q", "./g", "http://a/b/c/g"],
  ["http://a/b/c/d;p?q", "g/", "http://a/b/c/g/"],
  ["http://a/b/c/d;p?q", "/g", "http://a/g"],
  ["http://a/b/c/d;p?q", "//g", "http://g/"],
  ["http://a/b/c/d;p?q", "?y", "http://a/b/c/d;p?y"],
  ["http://a/b/c/d;p?q", "g?y", "http://a/b/c/g?y"],
  ["http://a/b/c/d;p?q", "#s", "http://a/b/c/d;p?q#s"],
  ["http://a/b/c/d;p?q", "g#s", "http://a/b/c/g#s"],
  ["http://a/b/c/d;p?q", "", "http://a/b/c/d;p?q"],
  ["http://a/b/c/d;p?q", ".", "http://a/b/c/"],
  ["http://a/b/c/d;p?q", "..", "http://a/b/"],
  ["http://a/b/c/d;p?q", "../g", "http://a/b/g"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("RFC parsed relative matrix passed");
