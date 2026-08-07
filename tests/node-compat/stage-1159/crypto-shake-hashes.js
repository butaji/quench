const crypto = require("crypto");

const shake128 = crypto
  .createHash("shake128", { outputLength: 16 })
  .digest("hex");
if (shake128 !== "7f9c2ba4e88f827d616045507605853e") {
  throw new Error(`unexpected SHAKE128 digest: ${shake128}`);
}

const shake256 = crypto
  .createHash("shake256", { outputLength: 32 })
  .digest("hex");
if (
  shake256 !==
    "46b9dd2b0ba88d13233b3feb743eeb243fcd52ea62b81b82b50c27646ed5762f"
) {
  throw new Error(`unexpected SHAKE256 digest: ${shake256}`);
}
