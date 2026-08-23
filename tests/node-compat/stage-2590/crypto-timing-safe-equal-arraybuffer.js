"use strict";

const assert = require("assert");
const crypto = require("crypto");

const left = new Uint8Array([0x71, 0x75, 0x65, 0x6e]);
const right = new Uint8Array([0x71, 0x75, 0x65, 0x6e]);
assert.strictEqual(crypto.timingSafeEqual(left.buffer, right.buffer), true);

const view = new DataView(new Uint8Array([0x71, 0x75, 0x65, 0x6e]).buffer);
const backing = new Uint8Array([0xaa, 0x71, 0x75, 0x65, 0x6e, 0xbb]).buffer;
const sliced = new DataView(backing, 1, 4);
assert.strictEqual(crypto.timingSafeEqual(left, sliced), true);
assert.strictEqual(crypto.timingSafeEqual(left, view), true);
assert.strictEqual(
  crypto.timingSafeEqual(
    left.buffer,
    new Uint8Array([0x71, 0x75, 0x65, 0x6f]).buffer,
  ),
  false,
);

console.log("ok");
