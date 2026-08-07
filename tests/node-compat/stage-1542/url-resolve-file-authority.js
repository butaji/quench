const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve(
    "file://meetings.example.com/cal#m1",
    "file:/devel/WWW/2000/10/swap/test/reluri-1.n3",
  ),
  "file:///cal#m1",
);
console.log("file URL authority resolution passed");
