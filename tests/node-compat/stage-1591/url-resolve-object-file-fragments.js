const assert = require("node:assert");
const url = require("node:url");

const cases = [
  [
    "file:/swap/test/animal.rdf",
    "#Animal",
    "file:/swap/test/animal.rdf#Animal",
  ],
  ["file:/some/dir/foo", "./#blort", "file:/some/dir/#blort"],
  ["file:/some/dir/foo", "./#", "file:/some/dir/#"],
  ["file:/ex/x/y", "q/r#s", "file:/ex/x/q/r#s"],
  ["file:/ex/x/y", "q/r#", "file:/ex/x/q/r#"],
  ["file:/ex/x/y", "", "file:/ex/x/y"],
  ["file:/ex/x/y/", "", "file:/ex/x/y/"],
  ["file:/ex/x/y/", "z/", "file:/ex/x/y/z/"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("file fragment matrix passed");
