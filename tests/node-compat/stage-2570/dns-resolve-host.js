const assert = require("node:assert");
const dns = require("node:dns");

dns.promises.resolve("localhost", "A").then((addresses) => {
  assert.ok(addresses.includes("127.0.0.1"));
  return dns.promises.resolve("localhost", "AAAA");
}).then((addresses) => {
  assert.ok(addresses.includes("::1"));
  console.log("DNS resolve passed");
});
