const fs = require("fs");
const assert = require("assert");
const path = `/tmp/quench-node-stage-129-${process.pid}`;
fs.writeFileSync(path, "hello ", { encoding: "utf8", flag: "a" });
fs.writeFileSync(path, "world!", { encoding: "utf8", flag: "a" });
assert.strictEqual(fs.readFileSync(path, "utf8"), "hello world!");
fs.writeFileSync(path, Buffer.from("4142", "hex"), { encoding: "hex" });
assert.strictEqual(fs.readFileSync(path, "utf8"), "AB");
fs.rmSync(path);
