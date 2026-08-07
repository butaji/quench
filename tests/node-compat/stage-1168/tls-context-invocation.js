const assert = require("assert");
const tls = require("tls");

const context = tls.createSecureContext().context;
assert.throws(() => ({ setOptions: context.setOptions }).setOptions(), {
  name: "TypeError",
  message: "Illegal invocation",
});
assert.throws(() => tls.createSecureContext({ pfx: Buffer.from("pfx") }), {
  name: "Error",
  message: "mac verify failure",
});
