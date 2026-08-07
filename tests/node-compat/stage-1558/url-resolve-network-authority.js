const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve("https://user:password@example.org/", "//another.host.com/"),
  "https://another.host.com/",
);
console.log("network authority resolution passed");
