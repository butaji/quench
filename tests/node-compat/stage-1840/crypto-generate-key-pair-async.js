const assert = require("assert");
const crypto = require("crypto");

let calls = 0;
crypto.generateKeyPair(
  "rsa",
  { modulusLength: 512 },
  (error, publicKey, privateKey) => {
    calls++;
    assert.ifError(error);
    assert.strictEqual(publicKey.type, "public");
    assert.strictEqual(privateKey.type, "private");
    assert.strictEqual(typeof publicKey.export, "function");
    assert.strictEqual(typeof privateKey.export, "function");
  },
);

setTimeout(() => assert.strictEqual(calls, 1), 0);
