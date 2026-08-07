const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["http://a/b/c/d;p=1/2?q", "./", "http://a/b/c/d;p=1/2/"],
  ["http://a/b/c/d;p=1/2?q", "../", "http://a/b/c/"],
  ["http://a/b/c/d;p=1/2?q", "../../", "http://a/b/"],
  ["http://a/b/c/d;p=1/2?q", "../../g", "http://a/b/g"],
  ["http://a/b/c/d;p=1/2?q", "g;x=1/./y", "http://a/b/c/d;p=1/2/g;x=1/y"],
  ["http://a/b/c/d;p=1/2?q", "g;x=1/../y", "http://a/b/c/d;p=1/2/y"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("path parameter directory matrix passed");
