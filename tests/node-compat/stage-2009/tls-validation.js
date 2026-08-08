const assert = require("assert");
const tls = require("tls");

assert.throws(() => tls.createSecureContext({ ciphers: 1 }), {
  code: "ERR_INVALID_ARG_TYPE"
});
assert.throws(() => tls.createServer({ ciphers: 1 }), {
  code: "ERR_INVALID_ARG_TYPE"
});
assert.throws(() => tls.connect({ checkServerIdentity: 1 }), {
  code: "ERR_INVALID_ARG_TYPE"
});
assert.throws(() => tls.connect({ checkServerIdentity: undefined }), {
  code: "ERR_INVALID_ARG_TYPE"
});
