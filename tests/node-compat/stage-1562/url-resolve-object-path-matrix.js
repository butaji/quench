const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["/foo/bar/baz", "quux/asdf", "/foo/bar/quux/asdf"],
  ["/foo/bar/baz", "../quux/baz", "/foo/quux/baz"],
  ["/foo/bar/baz", "/bar", "/bar"],
  ["/foo/bar/baz/", "quux", "/foo/bar/baz/quux"],
  ["/foo", ".", "/"],
  ["/foo/bar", "..", "/"],
];
for (const [from, to, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), to),
    url.parse(expected),
    `${from} + ${to}`,
  );
}
console.log("parsed path matrix passed");
