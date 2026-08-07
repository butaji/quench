const fs = require("fs");
const assert = require("assert");
const path = `/tmp/quench-node-stage-136-${process.pid}`;
fs.writeFileSync(path, Buffer.from("hello"));
assert.strictEqual(
  fs.readFileSync(path, "hex"),
  Buffer.from("hello").toString("hex"),
);
assert.strictEqual(
  fs.readFileSync(path, { encoding: "base64" }),
  Buffer.from("hello").toString("base64"),
);
fs.rmSync(path);
