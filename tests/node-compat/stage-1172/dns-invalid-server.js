const assert = require("assert");
const dns = require("dns");

assert.throws(() => dns.setServers(["invalid"]), {
  name: "TypeError",
  code: "ERR_INVALID_IP_ADDRESS",
});
