"use strict";

const assert = require("assert");
const common = require("../../tests/node/test/common");
const fs = require("fs");
const tmpdir = require("../../tests/node/test/common/tmpdir");
const { internalBinding } = require("internal/test/binding");
const { open, readFile } = fs.promises;
const binding = internalBinding("fs");

tmpdir.refresh();
const path = tmpdir.resolve("quench-stage-promises-buffer.txt");
const content = Buffer.from("Hello promises buffer option\n".repeat(128));
fs.writeFileSync(path, content);
let step = "start";

async function withFstatSizeZero(callback) {
  const original = binding.fstat;
  binding.fstat = function (...args) {
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
  const buffer = Buffer.alloc(content.length + 8, 0x78);
  const value = await readFile(path, { buffer });
  assert.deepStrictEqual(value, buffer.subarray(0, content.length));
  assert.deepStrictEqual(value, content);

  step = "read encoding";
  const encodedBuffer = Buffer.alloc(content.length + 8);
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
      return Buffer.alloc(fileSize + 4);
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
  const handleValue = await handle.readFile({
    buffer: Buffer.alloc(content.length + 8)
  });
  assert.deepStrictEqual(handleValue, content);
  await handle.close();

  step = "handle factory";
  const factoryHandle = await open(path, "r");
  let handleSize;
  const factoryHandleValue = await factoryHandle.readFile({
    buffer(fileSize) {
      handleSize = fileSize;
      return Buffer.alloc(fileSize + 4);
    }
  });
  assert.strictEqual(handleSize, content.length);
  assert.deepStrictEqual(factoryHandleValue, content);
  await factoryHandle.close();

  step = "using handle";
  {
    await using usingHandle = await open(path, "r");
    const usingValue = await usingHandle.readFile({
      buffer: Buffer.alloc(content.length + 8)
    });
    assert.deepStrictEqual(usingValue, content);
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
      const zeroBuffer = Buffer.alloc(content.length + 8, 0x78);
      const zeroValue = await readFile(path, {
        buffer: zeroBuffer
      });
      assert.deepStrictEqual(zeroValue, content);
      const zeroHandle = await open(path, "r");
      const zeroHandleValue = await zeroHandle.readFile({
        buffer: Buffer.alloc(content.length + 8)
      });
      assert.deepStrictEqual(zeroHandleValue, content);
      await zeroHandle.close();
      await using zeroUsingHandle = await open(path, "r");
      const zeroUsingValue = await zeroUsingHandle.readFile({
        buffer: Buffer.alloc(content.length + 8)
      });
      assert.deepStrictEqual(zeroUsingValue, content);
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
})()
  .then(common.mustCall())
  .catch((error) => {
    throw new Error(`step ${step}: ${error?.message || error}`);
  });
