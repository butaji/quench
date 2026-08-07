const assert = require("node:assert");
const crypto = require("node:crypto");

const { publicKey, privateKey } = crypto.generateKeyPairSync("rsa", {
  modulusLength: 1024,
});
for (const key of [publicKey, privateKey]) {
  assert(key instanceof crypto.KeyObject);
  assert.notStrictEqual(key.asymmetricKeyType, undefined);
  assert.deepStrictEqual(Reflect.ownKeys(key), []);
}
assert.strictEqual(publicKey.equals(publicKey), true);
console.log("crypto asymmetric key object basics passed");
