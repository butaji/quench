"use strict";

const assert = require("assert");
const dns = require("dns");

assert.deepStrictEqual(dns.getServers(), ["127.0.0.1"]);
dns.setServers(["8.8.8.8"]);
assert.deepStrictEqual(dns.getServers(), ["8.8.8.8"]);
const resolver = new dns.Resolver();
assert.deepStrictEqual(resolver.getServers(), ["8.8.8.8"]);
dns.lookup("localhost", (error, address, family) => {
  assert.ifError(error);
  assert.strictEqual(address, "127.0.0.1");
  assert.strictEqual(family, 4);
});

(async () => {
  assert.deepStrictEqual(await require("dns/promises").lookup("localhost"), {
    address: "127.0.0.1",
    family: 4,
  });
  console.log("dns passed");
})();
