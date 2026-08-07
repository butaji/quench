const assert = require("assert");
const dns = require("dns");

assert.throws(() => dns.resolve("example.com", []), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
});
