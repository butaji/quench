const assert = require("assert");
const fs = require("fs");
const net = require("net");

const source = fs.readFileSync(
  "tests/node/test/parallel/test-net-isipv6.js",
  "utf8"
);
const section = source.match(/const v6 = \[(.*?)\];/s)?.[1] || "";
const valid = [...section.matchAll(/'([^']*)'/g)].map((match) => match[1]);
assert(valid.length > 100);
for (const value of valid) {
  assert.strictEqual(net.isIPv6(value), true, `${value} was rejected`);
}
