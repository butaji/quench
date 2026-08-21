const assert = require("assert");

const assertUsages = async (keyPromise, expected) => {
  const key = await keyPromise;
  assert.deepStrictEqual(key.usages, expected);
  assert.deepStrictEqual(key.usages, expected);
};

assertUsages(
  crypto.subtle.generateKey({ name: "HMAC", hash: "SHA-256" }, true, [
    "verify",
    "sign",
    "verify",
    "sign",
  ]),
  ["sign", "verify"],
);

assertUsages(
  crypto.subtle.importKey(
    "raw",
    new Uint8Array(16),
    { name: "AES-GCM" },
    true,
    ["decrypt", "encrypt", "decrypt"],
  ),
  ["encrypt", "decrypt"],
);

console.log("webcrypto usage order passed");
