const assert = require("node:assert");
const fs = require("node:fs");
const tmpdir = require("../../node/test/common/tmpdir");

tmpdir.refresh();
const fileURL = tmpdir.fileURL("url-write.txt");
fs.writeFileSync(fileURL, "content");
assert.strictEqual(fs.readFileSync(fileURL, "utf8"), "content");

console.log("tmpdir file URL passed");
