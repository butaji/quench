const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["http://example.com?", "http://example.com/?"],
  ["http://example.com?foo=bar#frag", "http://example.com/?foo=bar#frag"],
  ["http://example.com?foo=@bar#frag", "http://example.com/?foo=@bar#frag"],
  ["http://example.com?foo=/bar/#frag", "http://example.com/?foo=/bar/#frag"],
  ["http://example.com#frag=?bar/#frag", "http://example.com/#frag=?bar/#frag"],
];
for (const [input, expected] of cases) {
  assert.strictEqual(url.format(url.parse(input)), expected, input);
}
console.log("legacy format matrix passed");
