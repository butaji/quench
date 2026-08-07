const assert = require("node:assert");
const url = require("node:url");

const base = "http://a/b/c/d;p?q";
const cases = [
  ["g#s", "http://a/b/c/g#s"],
  ["g?y#s", "http://a/b/c/g?y#s"],
  [";x", "http://a/b/c/;x"],
  ["g;x", "http://a/b/c/g;x"],
  ["g;x?y#s", "http://a/b/c/g;x?y#s"],
];
for (const [to, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(base), to),
    url.parse(expected),
    `${base} + ${to}`,
  );
}
console.log("parsed web suffixes passed");
