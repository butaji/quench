"use strict";

const assert = require("assert");
const { PassThrough } = require("stream");

const pass = new PassThrough();
const chunks = [];
pass.on("data", (chunk) => chunks.push(chunk));
pass.end(Buffer.from("through"));
assert.strictEqual(Buffer.concat(chunks).toString(), "through");
assert.strictEqual(pass.readable, true);
assert.strictEqual(pass.writable, false);

console.log("stream pass through passed");
