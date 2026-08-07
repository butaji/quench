const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["foo:a/b", "../c", "foo:c"],
  ["foo:a", "foo:.", "foo:"],
  ["zz:abc", "/foo/../../../bar", "zz:/bar"],
  ["zz:abc", "/foo/../bar", "zz:/bar"],
  ["zz:abc", "foo/../../../bar", "zz:bar"],
  ["zz:abc", "foo/../bar", "zz:bar"],
  ["zz:abc", "zz:.", "zz:"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("legacy opaque matrix passed");
