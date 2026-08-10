const assert = require("node:assert");
const dns = require("node:dns");

dns.reverse("127.0.0.1", (error, hostnames) => {
  assert.ifError(error);
  assert.deepStrictEqual(hostnames, ["localhost"]);
  dns.promises.reverse("127.0.0.1").then((result) => {
    assert.deepStrictEqual(result, ["localhost"]);
    console.log("DNS reverse passed");
  });
});
