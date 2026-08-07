const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["http://a/b/c/d;p?q", "/.", "http://a/"],
  ["http://a/b/c/d;p?q", "/.foo", "http://a/.foo"],
  ["http://a/b/c/d;p?q", ".foo", "http://a/b/c/.foo"],
  ["http://a/b/c/d;p?q", "/foo/../../../bar", "http://a/bar"],
  ["http://a/b/c/d;p?q", "/foo/../bar", "http://a/bar"],
  ["http://a/b/c/d;p?q", "/a/b/c/./../../g", "http://a/a/g"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("root dot-name matrix passed");
