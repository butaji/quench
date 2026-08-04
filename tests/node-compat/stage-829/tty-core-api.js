"use strict";

const assert = require("assert");
const ttyApi = require("node:tty");

assert.strictEqual(typeof ttyApi.isatty, "function");
assert.strictEqual(typeof ttyApi.ReadStream, "function");
assert.strictEqual(typeof ttyApi.WriteStream, "function");
assert.strictEqual(ttyApi.isatty(-1), false);

console.log("tty core api passed");
