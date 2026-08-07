const assert = require("node:assert");
const url = require("node:url");

const base = "http://a/b/c/d;p?q";
const cases = [
  ["g?y/./x", "http://a/b/c/g?y/./x"],
  ["g?y/../x", "http://a/b/c/g?y/../x"],
  ["g#s/./x", "http://a/b/c/g#s/./x"],
  ["g#s/../x", "http://a/b/c/g#s/../x"],
  ["http:g", "http://a/b/c/g"],
];
for (const [to, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(base), to),
    url.parse(expected),
    `${base} + ${to}`,
  );
}
console.log("parsed web query paths passed");
