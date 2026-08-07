const assert = require("node:assert");
const crypto = require("node:crypto");

const hash = crypto.createHash("sha256", { defaultEncoding: "latin1" });
let output;
hash.on("data", (value) => (output = value.toString("hex")));
hash.write("öäü");
hash.end();
assert.strictEqual(
  output,
  "cd37bccd5786e2e76d9b18c871e919e6eb11cc12d868f5ae41c40ccff8e44830",
);
console.log("crypto hash default encoding passed");
