const assert = require("assert");
const crypto = require("crypto");

const signature = crypto.sign("sha256", Buffer.from("data"), "private key");
assert.ok(Buffer.isBuffer(signature));
assert.strictEqual(
  crypto.verify("sha256", Buffer.from("data"), "public key", signature),
  true,
);

let callbacks = 0;
crypto.sign("sha256", Buffer.from("data"), "private key", (error, value) => {
  callbacks++;
  assert.ifError(error);
  assert.ok(Buffer.isBuffer(value));
});
crypto.verify(
  "sha256",
  Buffer.from("data"),
  "public key",
  signature,
  (error, value) => {
    callbacks++;
    assert.ifError(error);
    assert.strictEqual(value, true);
  },
);

setTimeout(() => assert.strictEqual(callbacks, 2), 0);
