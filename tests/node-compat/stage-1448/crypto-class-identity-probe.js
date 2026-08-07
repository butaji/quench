const assert = require("node:assert");
const crypto = require("node:crypto");

const cases = [
  ["Hash", () => crypto.createHash("sha1")],
  ["Hmac", () => crypto.createHmac("sha1", "key")],
  [
    "Cipheriv",
    () =>
      crypto.createCipheriv(
        "des-ede3-cbc",
        "0123456789abcd0123456789",
        "12345678",
      ),
  ],
  [
    "Decipheriv",
    () =>
      crypto.createDecipheriv(
        "des-ede3-cbc",
        "0123456789abcd0123456789",
        "12345678",
      ),
  ],
  ["Sign", () => crypto.createSign("RSA-SHA1")],
  ["Verify", () => crypto.createVerify("RSA-SHA1")],
  ["DiffieHellman", () => crypto.createDiffieHellman(1024)],
  ["DiffieHellmanGroup", () => crypto.createDiffieHellmanGroup("modp5")],
  ["ECDH", () => crypto.createECDH("prime256v1")],
];
for (const [name, create] of cases) {
  const value = create();
  console.log(name, value instanceof crypto[name]);
  assert(value instanceof crypto[name]);
}
