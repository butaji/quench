const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve("http://example.com/b//c//d;p?q#blarg", "http:/p/a/t/h?s#hash2"),
  "http://example.com/p/a/t/h?s#hash2",
);
console.log("url resolve same origin path passed");
