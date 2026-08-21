const fs = require("node:fs");
const crypto = require("node:crypto");

const source = fs.readFileSync(process.argv[1], "utf8");
const digest = crypto.createHash("sha256").update(source).digest("hex");
console.log(
  JSON.stringify({ bytes: Buffer.byteLength(source), sha256: digest }),
);
