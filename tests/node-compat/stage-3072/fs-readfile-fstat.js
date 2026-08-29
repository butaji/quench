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
  assert.deepStrictEqual(
    fs.readFileSync(path, { buffer: Buffer.alloc(content.length + 16, 0x78) }),
    content
  );
  assert.strictEqual(
    fs.readFileSync(path, {
      buffer: Buffer.alloc(content.length + 16),
      encoding: "utf8"
    }),
    content.toString()
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

  assert.deepStrictEqual(
    await readFile({ buffer: Buffer.alloc(content.length + 16, 0x78) }),
    content
  );
  assert.strictEqual(
    await readFile({
      encoding: "utf8",
      buffer: Buffer.alloc(content.length + 16)
    }),
    content.toString()
  );
  size = undefined;
  assert.deepStrictEqual(
    await readFile({
      buffer(fileSize) {
        size = fileSize;
        return Buffer.alloc(fileSize + 8);
      }
    }),
    content
  );
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
