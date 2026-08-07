const assert = require("assert");
const tls = require("tls");

let error;
try {
  tls.createSecureContext({ ciphers: 1 });
} catch (value) {
  error = value;
}
assert.strictEqual(error?.name, "TypeError");
assert.strictEqual(error?.code, "ERR_INVALID_ARG_TYPE");
assert.strictEqual(
  error?.message,
  'The "options.ciphers" property must be of type string. Received type number (1)',
);
