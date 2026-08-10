const assert = require("assert");

(async () => {
  const key = await crypto.subtle.generateKey(
    { name: "AES-GCM", length: 128 },
    true,
    ["encrypt", "decrypt"],
  );
  const iv = new Uint8Array(12);
  const data = new TextEncoder().encode("quench-node");
  const encrypted = await crypto.subtle.encrypt(
    { name: "AES-GCM", iv },
    key,
    data,
  );
  const decrypted = await crypto.subtle.decrypt(
    { name: "AES-GCM", iv },
    key,
    encrypted,
  );
  assert.deepStrictEqual(
    Array.from(new Uint8Array(decrypted)),
    Array.from(data),
  );
})();
