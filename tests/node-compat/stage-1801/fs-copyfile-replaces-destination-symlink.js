const assert = require("assert");
const fs = require("fs");
const path = require("path");

const root = fs.mkdtempSync(path.join(process.cwd(), "quench-copyfile-"));
const source = path.join(root, "copy-source.txt");
const target = path.join(root, "copy-target");
const directory = path.join(root, "copy-directory");

fs.mkdirSync(directory, { recursive: true });
fs.writeFileSync(source, "replacement");
try {
  fs.unlinkSync(target);
} catch (_) {}
fs.symlinkSync(directory, target, "dir");

fs.copyFileSync(source, target);

assert.strictEqual(fs.readFileSync(target, "utf8"), "replacement");
assert.ok(fs.statSync(target).isFile());
fs.rmSync(root, { recursive: true, force: true });
