const crypto = require("crypto");

for (
  const create of [
    () => crypto.createHash("sha256"),
    () => crypto.createHmac("sha256", "key"),
  ]
) {
  const digest = create().update("value");
  digest.digest();
  for (
    const operation of [
      () => digest.update("again"),
      () => digest.digest(),
    ]
  ) {
    let error;
    try {
      operation();
    } catch (caught) {
      error = caught;
    }
    if (!error || error.code !== "ERR_CRYPTO_HASH_FINALIZED") {
      throw new Error("finalized crypto object accepted another operation");
    }
  }
}

console.log("crypto finalized passed");
