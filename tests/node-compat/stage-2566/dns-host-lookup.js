const assert = require("assert");

const addresses = __quench_dns_lookup("localhost", 0);
assert.ok(Array.isArray(addresses));
assert.ok(addresses.length > 0);
assert.ok(
  addresses.some((address) => address === "127.0.0.1" || address === "::1"),
);

console.log("DNS host lookup passed");
