const assert = require("node:assert");
const fs = require("node:fs");

fs.writeFileSync("symlink-time-target", "target");
fs.symlinkSync("symlink-time-target", "symlink-time-link");
assert.notStrictEqual(
  fs.lstatSync("symlink-time-link").mtime.getTime(),
  fs.statSync("symlink-time-link").mtime.getTime(),
);

console.log("symlink stat times passed");
