const assert = require("node:assert");
const fs = require("node:fs");

const directory = "rm-read-only-parent";
const file = `${directory}/file.txt`;
fs.mkdirSync(directory);
fs.writeFileSync(file, "data");
fs.chmodSync(directory, 0o444);
assert.throws(() => fs.rmSync(file, { force: true }), { code: "EACCES" });
fs.chmodSync(directory, 0o755);
fs.rmSync(directory, { recursive: true, force: true });
console.log("rm read-only parent passed");
