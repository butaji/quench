const assert = require("assert");

(async () => {
  const key = await crypto.subtle.generateKey(
    { name: "HMAC", hash: "SHA-256" },
    true,
    ["sign", "verify"]
  );
  key.type;
  key.extractable;
  key.algorithm;
  key.usages;
  assert.deepStrictEqual(Object.getOwnPropertySymbols(key), []);
  assert.deepStrictEqual(Object.getOwnPropertyNames(key), []);
  assert.deepStrictEqual(Reflect.ownKeys(key), []);
})();
