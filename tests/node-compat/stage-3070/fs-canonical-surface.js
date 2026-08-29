'use strict';
const fs = require('fs');
const assert = require('assert');

for (const name of ['openSync', 'readSync', 'writeSync', 'closeSync']) {
  assert.strictEqual(typeof fs[name], 'function', name);
}
assert.strictEqual(fs, require('node:fs'));
const fd = fs.openSync('tests/node/test/fixtures/x.txt', 'r');
const buffer = new Uint8Array();
assert.throws(() => fs.readSync(fd, buffer, 0, 10, 0), {
  code: 'ERR_INVALID_ARG_VALUE',
  message: "The argument 'buffer' is empty and cannot be written. Received Uint8Array(0) []"
});
fs.closeSync(fd);

const fd2 = fs.openSync('tests/node/test/fixtures/x.txt', 'r');
try {
  fs.readSync(fd2, { buffer: null });
} catch (error) {
  assert.strictEqual(error.message, 'The "buffer" argument must be an instance of Buffer, TypedArray, or DataView. Received an instance of Object');
}
fs.closeSync(fd2);

const fd3 = fs.openSync('tests/node/test/fixtures/x.txt', 'r');
const check = (label, fn, code, message) => {
  try { fn(); assert.fail(label + ': no error'); } catch (error) {
    if (error.code !== code) assert.fail(`${label}: code=${error.code}`);
    if (message && error.message !== message) assert.fail(`${label}: message=${error.message}`);
  }
};
check('missing callback', () => fs.read(fd3, new Uint8Array(1), 0, 1, 0), 'ERR_INVALID_ARG_TYPE');
check('null options async', () => fs.read(fd3, { buffer: null }, () => {}), 'ERR_INVALID_ARG_TYPE');
check('null fd', () => fs.read(null, new Uint8Array(1), 0, 1, 0), 'ERR_INVALID_ARG_TYPE', 'The "fd" argument must be of type number. Received null');
fs.closeSync(fd3);
try { fs.accessSync(true); assert.fail('access boolean: no error'); } catch (error) {
  if (error.code !== 'ERR_INVALID_ARG_TYPE') assert.fail('access boolean: code=' + error.code);
  if (error.message !== 'The "path" argument must be of type string or an instance of Buffer or URL. Received type boolean (true)') assert.fail('access boolean: message=' + error.message);
}
