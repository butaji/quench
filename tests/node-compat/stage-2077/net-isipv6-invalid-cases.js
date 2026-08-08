const assert = require("assert");
const fs = require("fs");
const net = require("net");

const source = fs.readFileSync(
  "tests/node/test/parallel/test-net-isipv6.js",
  "utf8"
);
const section = source.match(/const v6not = \[(.*?)\];/s)?.[1] || "";
const invalid = [...section.matchAll(/'([^']*)'/g)].map((match) => match[1]);
assert(invalid.length > 80);
for (const value of invalid) {
  assert.strictEqual(net.isIPv6(value), false, `${value} was accepted`);
}
