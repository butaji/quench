const assert = require("node:assert");
const dns = require("node:dns");

Promise.all([
  dns.promises.resolve4("localhost"),
  dns.promises.resolve6("localhost"),
])
  .then(([ipv4, ipv6]) => {
    assert.ok(ipv4.includes("127.0.0.1"));
    assert.ok(ipv6.includes("::1"));
    console.log("DNS resolve4/resolve6 passed");
  });
