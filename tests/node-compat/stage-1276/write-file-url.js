const assert = require("node:assert");
const fs = require("node:fs");
const { pathToFileURL } = require("node:url");

const filename = pathToFileURL(
  `${process.cwd()}/tests/node/test/.tmp.0/url-write`,
);
fs.writeFileSync(filename, "url");
assert.strictEqual(fs.readFileSync(filename, "utf8"), "url");

console.log("writeFile URL passed");
