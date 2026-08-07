const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["http://a/b/c/d;p?q", "http:g", "http://a/b/c/g"],
  ["http://a/b/c/d;p?q", "http:", "http://a/b/c/d;p?q"],
  ["foo:a", "foo:.", "foo:"],
  ["foo:a/b", "foo:g", "foo:g"],
  ["http://a/b/c/d;p?q", "https:g", "https:g"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("same-scheme form matrix passed");
