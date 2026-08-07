"use strict";

const assert = require("assert");
const childProcess = require("child_process");

assert.strictEqual(childProcess.execSync("printf ok").toString(), "ok");
assert.strictEqual(
  childProcess.execFileSync("printf", ["%s", "file-ok"], {
    encoding: "utf8",
  }),
  "file-ok",
);
assert.strictEqual(
  childProcess.execFileSync("printf", ["%s", "buffer-ok"]) instanceof Buffer,
  true,
);

console.log("child process sync success passed");
