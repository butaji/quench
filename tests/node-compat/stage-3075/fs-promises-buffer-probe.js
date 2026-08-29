"use strict";

const assert = require("assert");
const common = require("../../tests/node/test/common");
const fs = require("fs");
const tmpdir = require("../../tests/node/test/common/tmpdir");
const { internalBinding } = require("internal/test/binding");
const { open, readFile } = fs.promises;
const binding = internalBinding("fs");

tmpdir.refresh();
const path = tmpdir.resolve("fs-promises-readfile-buffer-option.txt");
const content = Buffer.from("Hello promises buffer option\n".repeat(128));
fs.writeFileSync(path, content);
let step = "start";
let fstatCalls = 0;

async function withFstatSizeZero(callback) {
  const original = binding.fstat;
  binding.fstat = function (...args) {
    fstatCalls++;
    const result = Reflect.apply(original, this, args);
    return Promise.resolve(result).then((stats) => {
      if (stats !== undefined) stats[8] = 0;
      return stats;
    });
  };
  try {
    await callback();
  } finally {
    binding.fstat = original;
  }
}

(async () => {
  step = "read buffer";
  const buffer = Buffer.alloc(content.length + 16, 0x78);
  const value = await readFile(path, { buffer });
  assert.deepStrictEqual(value, buffer.subarray(0, content.length));
  assert.deepStrictEqual(value, content);
  assert(buffer.subarray(content.length).every((byte) => byte === 0x78));

  step = "read encoding";
  const encodedBuffer = Buffer.alloc(content.length + 16);
  assert.strictEqual(
    await readFile(path, { buffer: encodedBuffer, encoding: "utf8" }),
    content.toString("utf8")
  );
  assert.deepStrictEqual(encodedBuffer.subarray(0, content.length), content);

  step = "factory";
  let size;
  const factoryValue = await readFile(path, {
    buffer(fileSize) {
      size = fileSize;
      return Buffer.alloc(fileSize + 8);
    }
  });
  assert.strictEqual(size, content.length);
  assert.deepStrictEqual(factoryValue, content);
  step = "undersized";
  await assert.rejects(
    readFile(path, { buffer: Buffer.alloc(content.length - 1) }),
    { code: "ERR_INVALID_ARG_VALUE" }
  );
  await assert.rejects(
    readFile(path, {
      buffer() {
        return Buffer.alloc(content.length - 1);
      }
    }),
    { code: "ERR_INVALID_ARG_VALUE" }
  );

  step = "handle";
  const handle = await open(path, "r");
  let handleBuffer;
  const handleValue = await handle.readFile({
    buffer: (handleBuffer = Buffer.alloc(content.length + 16, 0x78))
  });
  assert.deepStrictEqual(handleValue, content);
  assert(handleBuffer.subarray(content.length).every((byte) => byte === 0x78));
  await handle.close();

  step = "handle factory";
  const factoryHandle = await open(path, "r");
  let handleSize;
  const factoryHandleValue = await factoryHandle.readFile({
    buffer(fileSize) {
      handleSize = fileSize;
      return Buffer.alloc(fileSize + 8);
    }
  });
  assert.strictEqual(handleSize, content.length);
  assert.deepStrictEqual(factoryHandleValue, content);
  await factoryHandle.close();

  step = "using handle";
  {
    await using usingHandle = await open(path, "r");
    const usingBuffer = Buffer.alloc(content.length + 16, 0x78);
    const usingValue = await usingHandle.readFile({
      buffer: usingBuffer
    });
    assert.deepStrictEqual(usingValue, content);
    assert(usingBuffer.subarray(content.length).every((byte) => byte === 0x78));
  }
  step = "handle undersized";
  {
    const undersizedHandle = await open(path, "r");
    await assert.rejects(
      undersizedHandle.readFile({ buffer: Buffer.alloc(content.length - 1) }),
      { code: "ERR_INVALID_ARG_VALUE" }
    );
    await undersizedHandle.close();
  }

  step = "zero-size fstat";
  await withFstatSizeZero(
    common.mustCall(async () => {
      const zeroBuffer = Buffer.alloc(content.length + 16, 0x78);
      const zeroValue = await readFile(path, {
        buffer: zeroBuffer
      });
      assert.deepStrictEqual(zeroValue, content);
      assert(zeroBuffer.subarray(content.length).every((byte) => byte === 0x78));
      const zeroHandle = await open(path, "r");
      let zeroHandleBuffer;
      const zeroHandleValue = await zeroHandle.readFile({
        buffer: (zeroHandleBuffer = Buffer.alloc(content.length + 16, 0x78))
      });
      assert.deepStrictEqual(zeroHandleValue, content);
      assert(zeroHandleBuffer.subarray(content.length).every((byte) => byte === 0x78));
      await zeroHandle.close();
      await using zeroUsingHandle = await open(path, "r");
      const zeroUsingBuffer = Buffer.alloc(content.length + 16, 0x78);
      const zeroUsingValue = await zeroUsingHandle.readFile({
        buffer: zeroUsingBuffer
      });
      assert.deepStrictEqual(zeroUsingValue, content);
      assert(zeroUsingBuffer.subarray(content.length).every((byte) => byte === 0x78));
      await assert.rejects(
        readFile(path, {
          buffer: Buffer.alloc(content.length - 1)
        }),
        { code: "ERR_INVALID_ARG_VALUE" }
      );
      await using zeroUndersizedHandle = await open(path, "r");
      await assert.rejects(
        zeroUndersizedHandle.readFile({
          buffer: Buffer.alloc(content.length - 1)
        }),
        { code: "ERR_INVALID_ARG_VALUE" }
      );
    })
  );
  assert(fstatCalls > 0);
})()
  .then(common.mustCall())
  .catch((error) => {
    throw new Error(`step ${step}: ${error?.message || error}`);
  });
