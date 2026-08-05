const assert = require("node:assert");
const fs = require("node:fs");

const fd = fs.openSync("write-string-overload.txt", "w+");
assert.strictEqual(fs.writeSync(fd, "ok", 0, "utf8"), 2);
fs.closeSync(fd);
assert.strictEqual(fs.readFileSync("write-string-overload.txt", "utf8"), "ok");
console.log("write string overload passed");
