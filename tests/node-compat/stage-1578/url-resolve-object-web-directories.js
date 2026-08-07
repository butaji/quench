const assert = require("node:assert");
const url = require("node:url");

const base = "http://a/b/c/d;p?q";
const cases = [
  ["", "http://a/b/c/d;p?q"],
  [".", "http://a/b/c/"],
  ["./", "http://a/b/c/"],
  ["..", "http://a/b/"],
  ["../", "http://a/b/"],
  ["../g", "http://a/b/g"],
  ["../../g", "http://a/g"],
  ["/./g", "http://a/g"],
  ["/../g", "http://a/g"],
];
for (const [to, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(base), to),
    url.parse(expected),
    `${base} + ${to}`,
  );
}
console.log("parsed web directories passed");
