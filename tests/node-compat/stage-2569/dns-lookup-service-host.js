const assert = require("node:assert");
const dns = require("node:dns");

dns.lookupService("127.0.0.1", 80, (error, hostname, service) => {
  assert.ifError(error);
  assert.strictEqual(hostname, "localhost");
  assert.strictEqual(service, "tcp");
  dns.promises.lookupService("127.0.0.1", 80).then((result) => {
    assert.deepStrictEqual(result, { hostname: "localhost", service: "tcp" });
    console.log("DNS lookupService passed");
  });
});
