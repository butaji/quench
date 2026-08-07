const assert = require("node:assert");
const url = require("node:url");

const base = "http://a/b/c/d;p?q";
const cases = [
  ["/.", "http://a/"],
  ["/.foo", "http://a/.foo"],
  [".foo", "http://a/b/c/.foo"],
  ["g", "http://a/b/c/g"],
  ["./g", "http://a/b/c/g"],
  ["g/", "http://a/b/c/g/"],
  ["/g", "http://a/g"],
  ["//g", "http://g/"],
  ["?y", "http://a/b/c/d;p?y"],
];
for (const [to, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(base), to),
    url.parse(expected),
    `${base} + ${to}`,
  );
}
console.log("parsed web RFC matrix passed");
