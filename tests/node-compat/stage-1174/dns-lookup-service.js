const assert = require("assert");
const dns = require("dns");

assert.throws(() => dns.lookupService("0.0.0.0"), {
  code: "ERR_MISSING_ARGS",
  name: "TypeError",
});
assert.throws(() => dns.promises.lookupService("0.0.0.0"), {
  code: "ERR_MISSING_ARGS",
  name: "TypeError",
});
