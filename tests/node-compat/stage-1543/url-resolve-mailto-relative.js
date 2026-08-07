const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve("more/qual2@domain2.org#frag", "mailto:local/qual1@domain1.org"),
  "mailto:local/more/qual2@domain2.org#frag",
);
console.log("mailto relative resolution passed");
