const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["/foo/bar?baz=quux#frag", { baz: "quux" }],
  ["http://example.com", {}],
  ["/example", {}],
  ["/example?query=value", { query: "value" }],
  ["/a?x=1&x=2&empty=", { x: ["1", "2"], empty: "" }],
];
for (const [input, expected] of cases) {
  assert.deepStrictEqual(url.parse(input, true).query, expected, input);
}
console.log("query object matrix passed");
