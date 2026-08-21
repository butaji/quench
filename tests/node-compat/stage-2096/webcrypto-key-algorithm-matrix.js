const assert = require("assert");

(async () => {
  const keys = [];
  keys.push(
    await crypto.subtle.generateKey({ name: "HMAC", hash: "SHA-256" }, true, [
      "sign",
      "verify",
    ]),
  );
  keys.push(
    (
      await crypto.subtle.generateKey(
        { name: "ECDSA", namedCurve: "P-256" },
        true,
        ["sign", "verify"],
      )
    ).privateKey,
  );
  keys.push(
    (
      await crypto.subtle.generateKey(
        {
          name: "RSA-PSS",
          modulusLength: 2048,
          publicExponent: new Uint8Array([1, 0, 1]),
          hash: "SHA-256",
        },
        true,
        ["sign", "verify"],
      )
    ).publicKey,
  );
  keys.push(
    await crypto.subtle.generateKey({ name: "AES-GCM", length: 128 }, true, [
      "encrypt",
      "decrypt",
    ]),
  );
  assert.strictEqual(keys.length, 4);
})();
