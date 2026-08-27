"use strict";
const assert = require("assert");
const { spawn } = require("child_process");

const child = spawn("cat");
assert.strictEqual(child.signalCode, null);
assert.strictEqual(child.killed, false);
assert.strictEqual(typeof child[Symbol.dispose], "function");
child[Symbol.dispose]();
assert.strictEqual(child.killed, true);
assert.strictEqual(child.signalCode, "SIGTERM");
