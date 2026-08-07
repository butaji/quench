const crypto = require("crypto");

const expected = {
  sha1: "8308651804facb7b9af8ffc53a33a22d6a1c8ac2",
  sha256: "d9b5f58f0b38198293971865a14074f59eba3e82595becbe86ae51f1d9f1f65e",
};
if (
  crypto.createHash("sha1").update("Test123").digest("hex") !== expected.sha1
) {
  throw new Error("sha1 digest mismatch");
}
if (
  crypto.createHash("sha256").update("Test123").digest("hex") !==
    expected.sha256
) {
  throw new Error("sha256 digest mismatch");
}
