"use strict";

const assert = require("assert");
const decoderApi = require("node:string_decoder");

assert.strictEqual(typeof decoderApi.StringDecoder, "function");
const decoder = new decoderApi.StringDecoder("utf8");
assert.strictEqual(typeof decoder.write, "function");
assert.strictEqual(typeof decoder.end, "function");
assert.strictEqual(decoder.write(Buffer.from("ok")), "ok");

console.log("string decoder api passed");
