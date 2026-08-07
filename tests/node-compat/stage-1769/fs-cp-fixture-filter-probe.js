const assert = require("assert");
const fs = require("fs");
const path = require("path");

const source = path.join(
  process.cwd(),
  "tests/node/test/fixtures/copy/kitchen-sink",
);
const destination = path.join(process.cwd(), "tests/node/test/.tmp.0/cp-probe");
fs.rmSync(destination, { recursive: true, force: true });
fs.cpSync(source, destination, {
  recursive: true,
  dereference: true,
  filter: (entry) => fs.statSync(entry).isDirectory() || entry.endsWith(".js"),
});
assert(fs.existsSync(path.join(destination, "index.js")));
assert(!fs.existsSync(path.join(destination, "README.md")));
console.log("fixture filter probe passed");
