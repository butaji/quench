const assert = require("node:assert");
const crypto = require("node:crypto");

const hash = crypto.createHash("sha256", { defaultEncoding: "latin1" });
let output;
let ended = false;
hash.on("data", (value) => (output = value.toString("hex")));
hash.on("end", () => (ended = true));
hash.write("compatibility");
hash.end();
assert.strictEqual(hash._writableState.defaultEncoding, "latin1");
assert.strictEqual(ended, true);
assert.strictEqual(
  output,
  crypto.createHash("sha256").update("compatibility").digest("hex"),
);
console.log("crypto hash stream passed");
