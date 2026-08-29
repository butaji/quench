"use strict";
const assert = require("assert");
const common = require("../../tests/node/test/common");
const fs = require("fs");
const tmpdir = require("../../tests/node/test/common/tmpdir");
const tick = require("../../tests/node/test/common/tick");

tmpdir.refresh();
assert.strictEqual(typeof tmpdir.hasEnoughSpace, "function");
assert.strictEqual(tmpdir.hasEnoughSpace(2 ** 31 - 1), false);
assert.strictEqual(typeof tick, "function");
tick(1, () => {});
common.printSkipMessage("space probe");

const path = tmpdir.resolve("readfile-abort.txt");
fs.writeFileSync(path, Buffer.from("value"));
const signal = AbortSignal.abort();
assert.strictEqual(signal.aborted, true);
new Promise((resolve, reject) => {
  fs.readFile(path, { signal }, common.mustCall((error) => {
    try {
      assert.strictEqual(error.name, "AbortError");
      resolve();
    } catch (reason) {
      reject(reason);
    }
  }));
}).then(common.mustCall());

const controller = new AbortController();
new Promise((resolve, reject) => {
  fs.readFile(path, { signal: controller.signal }, common.mustCall((error) => {
    try {
      assert.strictEqual(error.name, "AbortError");
      resolve();
    } catch (reason) {
      reject(reason);
    }
  }));
  process.nextTick(() => controller.abort());
}).then(common.mustCall());
