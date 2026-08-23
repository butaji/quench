"use strict";

const assert = require("assert");
const os = require("os");

assert.strictEqual(os.devNull, process.platform === "win32" ? "NUL" : "/dev/null");
console.log("os devNull fallback passed");
