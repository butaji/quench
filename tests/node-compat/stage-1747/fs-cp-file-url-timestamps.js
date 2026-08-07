const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const root = path.join(process.cwd(), "tests/node/test/.tmp.0/cp-url");
fs.rmSync(root, { recursive: true, force: true });
fs.mkdirSync(root, { recursive: true });
const source = path.join(root, "source.txt");
const destination = path.join(root, "destination.txt");
fs.writeFileSync(source, "timestamped");
const timestamp = new Date("2020-01-02T03:04:05.000Z");
fs.utimesSync(source, timestamp, timestamp);
fs.cpSync(new URL(`file://${source}`), new URL(`file://${destination}`), {
  preserveTimestamps: true,
});
console.log(
  `dest=${fs.readFileSync(destination, "utf8")} mtime=${
    fs.statSync(destination).mtimeMs
  }`,
);
assert.strictEqual(fs.readFileSync(destination, "utf8"), "timestamped");
assert.ok(
  Math.abs(fs.statSync(destination).mtimeMs - timestamp.getTime()) < 1000,
);
console.log("fs cp file URL timestamps passed");
