"use strict";

const assert = require("assert");
const fs = require("fs");
const path = "/tmp/quench-stage-readfile-fd.txt";
const content = Buffer.from("fd read contract\n");

fs.writeFileSync(path, content);
const fd = fs.openSync(path, "r");
try {
  assert.strictEqual(fs.readFileSync(fd, "utf8"), content.toString("utf8"));
} finally {
  fs.closeSync(fd);
}
