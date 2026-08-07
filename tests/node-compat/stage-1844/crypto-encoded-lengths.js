const crypto = require("crypto");
const pair = crypto.generateKeyPairSync("dsa", {
  modulusLength: 2048,
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "der" },
});
if (typeof pair.publicKey !== "string" || !pair.privateKey?.length) {
  throw new Error(
    `unexpected encoded lengths ${typeof pair
      .publicKey}/${pair.privateKey?.length}`,
  );
}
console.log(
  `crypto encoded lengths passed: ${pair.publicKey.length}/${pair.privateKey.length}`,
);
