const assert = require("assert");

(async () => {
  const pair = await crypto.subtle.generateKey(
    {
      name: "RSA-PSS",
      modulusLength: 2048,
      publicExponent: new Uint8Array([1, 0, 1]),
      hash: "SHA-256",
    },
    true,
    ["sign", "verify"],
  );
  const data = new TextEncoder().encode("quench-node");
  const signature = await crypto.subtle.sign(
    { name: "RSA-PSS", saltLength: 32 },
    pair.privateKey,
    data,
  );
  assert.strictEqual(
    await crypto.subtle.verify(
      { name: "RSA-PSS", saltLength: 32 },
      pair.publicKey,
      signature,
      data,
    ),
    true,
  );
})();
