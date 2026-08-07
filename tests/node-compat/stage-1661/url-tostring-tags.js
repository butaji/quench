const assert = require("node:assert");

const toString = Object.prototype.toString;
const url = new URL("http://example.org");
const searchParams = url.searchParams;
const iterator = searchParams.entries();

for (
  const [value, expected] of [
    [url, "URL"],
    [searchParams, "URLSearchParams"],
    [iterator, "URLSearchParams Iterator"],
    [Object.getPrototypeOf(url), "URL"],
    [Object.getPrototypeOf(searchParams), "URLSearchParams"],
    [Object.getPrototypeOf(iterator), "URLSearchParams Iterator"],
  ]
) {
  assert.strictEqual(value[Symbol.toStringTag], expected);
  assert.strictEqual(toString.call(value), `[object ${expected}]`);
}
console.log("URL toString tags passed");
