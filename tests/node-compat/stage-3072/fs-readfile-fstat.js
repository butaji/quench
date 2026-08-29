"use strict";

const assert = require("assert");
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
  const stats = Reflect.apply(original, this, args);
  if (stats !== undefined) stats[8] = 0;
  return stats;
};

new Promise((resolve, reject) => {
  fs.readFile(
    path,
    { buffer: Buffer.alloc(content.length + 16, 0x78) },
    (error, value) => {
      if (error) reject(error);
      else {
        assert.deepStrictEqual(value, content);
        resolve();
      }
    }
  );
})
  .then(() => {
    assert.strictEqual(calls, 1, "async readFile observes internal fs.fstat");
    const target = Buffer.alloc(content.length + 16, 0x78);
    assert.deepStrictEqual(fs.readFileSync(path, { buffer: target }), content);
    assert.strictEqual(
      fs.readFileSync(path, { buffer: target, encoding: "utf8" }),
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
    assert.throws(
      () => fs.readFileSync(path, { buffer: Buffer.alloc(content.length - 1) }),
      { code: "ERR_INVALID_ARG_VALUE" }
    );
  })
  .finally(() => {
    fs.unlinkSync(path);
  });
