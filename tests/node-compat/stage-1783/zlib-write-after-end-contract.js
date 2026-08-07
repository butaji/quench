"use strict";
const assert = require("assert");
const zlib = require("zlib");
const { test } = require("node:test");

test("write callbacks after readable end", async (t) => {
  const { promise, resolve } = Promise.withResolvers();
  const data = zlib.deflateRawSync("Welcome");
  const inflate = zlib.createInflateRaw();
  const writeCallback = t.mock.fn();
  inflate.resume();
  inflate.write(data, writeCallback);
  inflate.write(Buffer.from([0x00]), writeCallback);
  inflate.write(Buffer.from([0x00]), writeCallback);
  inflate.flush(resolve);
  await promise;
  assert.strictEqual(writeCallback.mock.callCount(), 3);
});
console.log("zlib write after end contract passed");
