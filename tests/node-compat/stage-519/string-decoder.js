"use strict";

const assert = require("assert");
const { StringDecoder } = require("string_decoder");

const decoder = new StringDecoder("utf8");
const bytes = Buffer.from("€uro");
assert.strictEqual(decoder.write(bytes.subarray(0, 1)), "");
assert.strictEqual(decoder.write(bytes.subarray(1, 3)), "€");
assert.strictEqual(decoder.end(bytes.subarray(3)), "uro");
assert.strictEqual(new StringDecoder().end(Buffer.from("done")), "done");

console.log("string decoder passed");
