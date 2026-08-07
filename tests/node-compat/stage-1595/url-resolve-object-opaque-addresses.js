const assert = require("node:assert");
const url = require("node:url");

const cases = [
  [
    "mid:m@example.ord/c@example.org",
    "m2@example.ord/c2@example.org",
    "mid:m@example.ord/m2@example.ord/c2@example.org",
  ],
  ["foo:a/b", "c/d", "foo:a/c/d"],
  ["foo:a/b", "/c/d", "foo:/c/d"],
  ["foo:a/b?c#d", "", "foo:a/b?c"],
  ["foo:a", "b/c", "foo:b/c"],
  ["foo:/a/y/z", "../b/c", "foo:/a/b/c"],
  ["foo:a", "./b/c", "foo:b/c"],
  ["foo:a", "/./b/c", "foo:/b/c"],
  ["foo://a//b/c", "../../d", "foo://a/d"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("opaque address matrix passed");
