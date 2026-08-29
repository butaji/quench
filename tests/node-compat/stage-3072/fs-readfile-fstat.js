"use strict";

const assert = require("assert");
const common = require("../../tests/node/test/common");
const fs = require("fs");
const { internalBinding } = require("internal/test/binding");

const binding = internalBinding("fs");
const original = binding.fstat;
const path = "/tmp/quench-stage-readfile-buffer-option.txt";
const content = Buffer.from("Hello buffer option\n".repeat(128));
fs.writeFileSync(path, content);
let calls = 0;
binding.fstat = function (...args) {
  calls++;
  const request = args[2];
  if (request?.oncomplete) {
    const originalOncomplete = request.oncomplete;
    request.oncomplete = function (error, stats) {
      if (!error) stats[8] = 0;
      return originalOncomplete.call(this, error, stats);
    };
    return Reflect.apply(original, this, args);
  }
  const stats = Reflect.apply(original, this, args);
  if (stats !== undefined) stats[8] = 0;
  return stats;
};

const readFile = (options) =>
  new Promise((resolve, reject) => {
    fs.readFile(path, options, (error, value) =>
      error ? reject(error) : resolve(value)
    );
  });

const scenario = common.mustCall(async () => {
  const syncBuffer = Buffer.alloc(content.length + 16, 0x78);
  const syncValue = fs.readFileSync(path, { buffer: syncBuffer });
  assert.deepStrictEqual(syncValue, syncBuffer.subarray(0, content.length));
  assert.deepStrictEqual(syncValue, content);
  assert(syncBuffer.subarray(content.length).every((byte) => byte === 0x78));
  const syncEncodedBuffer = Buffer.alloc(content.length + 16);
  assert.strictEqual(
    fs.readFileSync(path, { buffer: syncEncodedBuffer, encoding: "utf8" }),
    content.toString()
  );
  assert.deepStrictEqual(
    syncEncodedBuffer.subarray(0, content.length),
    content
  );
  let size;
  assert.deepStrictEqual(
    fs.readFileSync(path, {
      buffer(fileSize) {
        size = fileSize;
        return Buffer.alloc(fileSize + 8);
      }
    }),
    content
  );
  assert.strictEqual(size, content.length);

  const asyncBuffer = Buffer.alloc(content.length + 16, 0x78);
  const asyncValue = await readFile({ buffer: asyncBuffer });
  assert.deepStrictEqual(asyncValue, asyncBuffer.subarray(0, content.length));
  assert.deepStrictEqual(asyncValue, content);
  assert(asyncBuffer.subarray(content.length).every((byte) => byte === 0x78));
  const asyncEncodedBuffer = Buffer.alloc(content.length + 16);
  assert.strictEqual(
    await readFile({ encoding: "utf8", buffer: asyncEncodedBuffer }),
    content.toString()
  );
  assert.deepStrictEqual(
    asyncEncodedBuffer.subarray(0, content.length),
    content
  );
  size = undefined;
  let asyncFactoryBuffer;
  assert.deepStrictEqual(
    await readFile({
      buffer(fileSize) {
        size = fileSize;
        asyncFactoryBuffer = Buffer.alloc(fileSize + 8);
        return asyncFactoryBuffer;
      }
    }),
    asyncFactoryBuffer.subarray(0, content.length)
  );
  assert.deepStrictEqual(asyncFactoryBuffer.subarray(0, content.length), content);
  assert.strictEqual(size, content.length);

  assert.throws(
    () => fs.readFileSync(path, { buffer: Buffer.alloc(content.length - 1) }),
    { code: "ERR_INVALID_ARG_VALUE" }
  );
  await assert.rejects(readFile({ buffer: Buffer.alloc(content.length - 1) }), {
    code: "ERR_INVALID_ARG_VALUE"
  });
  assert.strictEqual(calls, 5, "async readFile observes internal fs.fstat");
});
scenario()
  .then(common.mustCall())
  .finally(() => {
    fs.unlinkSync(path);
  });
