const assert = require("node:assert");
const fs = require("node:fs");

const entries = fs.readdirSync(".", "hex");
assert.ok(entries.every((entry) => entry instanceof Buffer));
assert.deepStrictEqual(Buffer.from(".").toString("hex"), "2e");
console.log("Filesystem readdir hex encoding passed");
