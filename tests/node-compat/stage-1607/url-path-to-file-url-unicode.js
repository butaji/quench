const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["/fóóbàr", "file:///f%C3%B3%C3%B3b%C3%A0r"],
  ["/€", "file:///%E2%82%AC"],
  ["/🚀", "file:///%F0%9F%9A%80"],
  ["/foo\bbar", "file:///foo%08bar"],
  ["/foo\tbar", "file:///foo%09bar"],
  ["/foo\nbar", "file:///foo%0Abar"],
  ["/foo\rbar", "file:///foo%0Dbar"],
];
for (const [path, expected] of cases) {
  assert.strictEqual(url.pathToFileURL(path).href, expected, path);
}
console.log("Unicode file URL matrix passed");
