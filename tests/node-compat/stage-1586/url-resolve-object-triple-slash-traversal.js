const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["fred:///s//a/b/c", "./", "fred:///s//a/b/"],
  ["fred:///s//a/b/c", "../", "fred:///s//a/"],
  ["fred:///s//a/b/c", "../g", "fred:///s//a/g"],
  ["fred:///s//a/b/c", "../../", "fred:///s//"],
  ["fred:///s//a/b/c", "../../g", "fred:///s//g"],
  ["fred:///s//a/b/c", "../../../g", "fred:///s/g"],
  ["fred:///s//a/b/c", "../../../../g", "fred:///g"],
  ["http:///s//a/b/c", "./", "http:///s//a/b/"],
  ["http:///s//a/b/c", "../g", "http:///s//a/g"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("triple-slash traversal matrix passed");
