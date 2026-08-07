"use strict";
const assert = require("assert");
const zlib = require("zlib");

const inflate = zlib.createInflateRaw();
let calls = 0;
inflate.resume();
inflate.write(Buffer.from([0x01]), () => calls++);
inflate.write(Buffer.from([0x02]), () => calls++);
inflate.flush(() => {
  assert.strictEqual(calls, 2);
  console.log("zlib write callbacks passed");
});
