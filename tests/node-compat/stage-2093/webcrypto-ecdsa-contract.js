const assert = require("assert");

(async () => {
  const pair = await crypto.subtle.generateKey(
    { name: "ECDSA", namedCurve: "P-256" },
    true,
    ["sign", "verify"],
  );
  assert.strictEqual(pair.privateKey.type, "private");
  assert.strictEqual(pair.publicKey.type, "public");
  const data = new TextEncoder().encode("quench-node");
  const signature = await crypto.subtle.sign(
    { name: "ECDSA", hash: "SHA-256" },
    pair.privateKey,
    data,
  );
  assert.strictEqual(
    await crypto.subtle.verify(
      { name: "ECDSA", hash: "SHA-256" },
      pair.publicKey,
      signature,
      data,
    ),
    true,
  );
})();
