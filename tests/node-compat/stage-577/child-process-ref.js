"use strict";

const assert = require("assert");
const childProcess = require("child_process");

const child = childProcess.spawn("node", ["-e", ""]);
assert.strictEqual(typeof child.ref, "function");
assert.strictEqual(typeof child.unref, "function");
assert.strictEqual(child.ref(), child);
assert.strictEqual(child.unref(), child);
child.kill();

console.log("child process ref passed");
