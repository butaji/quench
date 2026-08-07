const assert = require("assert");
const fs = require("fs");
const path = require("path");

const root = fs.mkdtempSync(path.join(process.cwd(), "quench-cp-symlink-"));
const source = path.join(root, "source");
const destination = path.join(root, "destination");
fs.mkdirSync(source);
fs.writeFileSync(path.join(source, "file"), "content");
fs.symlinkSync(path.join(source, "file"), path.join(source, "link"));

fs.cpSync(source, destination, { recursive: true });
fs.cpSync(source, destination, { recursive: true });

assert.strictEqual(
  fs.readlinkSync(path.join(destination, "link")),
  path.join(source, "file"),
);
fs.rmSync(root, { recursive: true, force: true });
