const assert = require("assert");
const fs = require("fs");
const path = require("path");

const root = fs.mkdtempSync(path.join(process.cwd(), "quench-cp-force-"));
const source = path.join(root, "source");
const destination = path.join(root, "destination");
fs.writeFileSync(source, "source");
fs.writeFileSync(destination, "destination");

fs.cpSync(source, destination, { force: false });

assert.strictEqual(fs.readFileSync(destination, "utf8"), "destination");
fs.rmSync(root, { recursive: true, force: true });
