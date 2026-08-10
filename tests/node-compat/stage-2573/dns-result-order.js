const assert = require("node:assert");
const dns = require("node:dns");

assert.strictEqual(dns.getDefaultResultOrder(), "verbatim");
dns.setDefaultResultOrder("ipv4first");
assert.strictEqual(dns.getDefaultResultOrder(), "ipv4first");
assert.throws(() => dns.setDefaultResultOrder("invalid"), {
  code: "ERR_INVALID_ARG_VALUE",
});
const resolver = new dns.Resolver();
resolver.setDefaultResultOrder("ipv6first");
assert.strictEqual(resolver.getDefaultResultOrder(), "ipv6first");
console.log("DNS result order passed");
