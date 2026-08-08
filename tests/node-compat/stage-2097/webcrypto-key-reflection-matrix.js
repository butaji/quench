const assert = require("assert");

(async () => {
  const keys = [
    await crypto.subtle.generateKey({ name: "HMAC", hash: "SHA-256" }, true, [
      "sign",
      "verify"
    ]),
    (
      await crypto.subtle.generateKey(
        { name: "ECDSA", namedCurve: "P-256" },
        true,
        ["sign", "verify"]
      )
    ).privateKey,
    (
      await crypto.subtle.generateKey(
        {
          name: "RSA-PSS",
          modulusLength: 2048,
          publicExponent: new Uint8Array([1, 0, 1]),
          hash: "SHA-256"
        },
        true,
        ["sign", "verify"]
      )
    ).publicKey,
    await crypto.subtle.generateKey({ name: "AES-GCM", length: 128 }, true, [
      "encrypt",
      "decrypt"
    ])
  ];
  for (const key of keys) {
    key.type;
    key.extractable;
    key.algorithm;
    key.usages;
    assert.deepStrictEqual(Object.getOwnPropertySymbols(key), []);
    assert.deepStrictEqual(Object.getOwnPropertyNames(key), []);
    assert.deepStrictEqual(Reflect.ownKeys(key), []);
  }
})();
