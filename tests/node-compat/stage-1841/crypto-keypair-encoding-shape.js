const crypto = require("crypto");

const pair = crypto.generateKeyPairSync("dsa", {
  modulusLength: 2048,
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "der" },
});
if (
  typeof pair.publicKey !== "string" ||
  !pair.publicKey.includes("BEGIN PUBLIC KEY")
) {
  throw new Error("missing encoded public key");
}
if (!(pair.privateKey instanceof Buffer)) {
  throw new Error("missing DER private key");
}
console.log("crypto keypair encoding shape passed");
