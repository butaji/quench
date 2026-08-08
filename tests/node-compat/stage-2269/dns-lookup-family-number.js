const assert = require("assert");
const dns = require("dns");

dns.lookup("localhost", 4, (error, address, family) => {
  assert.ifError(error);
  assert.strictEqual(address, "127.0.0.1");
  assert.strictEqual(family, 4);
  console.log("dns lookup family number passed");
});
