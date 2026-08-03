"use strict";

const assert = require("assert");
const tty = require("tty");

assert.strictEqual(tty.isatty(1), false);
const output = new tty.WriteStream(1);
assert.strictEqual(output.isTTY, false);
assert.strictEqual(output.getColorDepth(), 1);
assert.strictEqual(output.hasColors(), false);
assert.deepStrictEqual(output.getWindowSize(), [0, 0]);
assert.strictEqual(new tty.ReadStream(0).isTTY, false);

console.log("tty surface passed");
