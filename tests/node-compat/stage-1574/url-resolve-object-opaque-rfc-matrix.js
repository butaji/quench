const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["zz:abc", "foo/../../../bar", "zz:bar"],
  ["zz:abc", "foo/../bar", "zz:bar"],
  ["zz:abc", "zz:.", "zz:"],
  ["foo:a/y/z", "../b/c", "foo:a/b/c"],
  ["foo:/a/y/z", "../b/c", "foo:/a/b/c"],
];
for (const [from, to, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), to),
    url.parse(expected),
    `${from} + ${to}`,
  );
}
console.log("parsed opaque RFC matrix passed");
