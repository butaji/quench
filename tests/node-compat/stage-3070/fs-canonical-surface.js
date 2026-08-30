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
const realpath = fs.realpathSync('tests/node/test/fixtures', { encoding: 'hex' });
assert.strictEqual(realpath, Buffer.from(fs.realpathSync('tests/node/test/fixtures')).toString('hex'));
const canonical = fs.realpathSync('tests/node/test/fixtures');
assert.strictEqual(fs.realpathSync('tests/node/test/fixtures', 'hex'), Buffer.from(canonical).toString('hex'));
const pathBuffer = Buffer.from(canonical);
assert.strictEqual(fs.realpathSync(pathBuffer, { encoding: 'hex' }), pathBuffer.toString('hex'));
fs.realpath('tests/node/test/fixtures', 'hex', (error, result) => {
  assert.ifError(error);
  assert.strictEqual(result, Buffer.from(canonical).toString('hex'));
});
for (const encoding of ['ascii', 'utf8', 'utf16le', 'ucs2', 'base64', 'binary', 'hex']) {
  fs.realpath(pathBuffer, encoding, (error, result) => {
    assert.ifError(error);
    assert.deepStrictEqual(result, pathBuffer.toString(encoding), encoding + ' async buffer');
  });
}
const content = Buffer.from('xyz\n');
const target = Buffer.alloc(content.length + 2, 0x78);
const result = fs.readFileSync('tests/node/test/fixtures/x.txt', { buffer: target });
assert.deepStrictEqual(result, target.subarray(0, content.length));
assert.deepStrictEqual(result, content);
(async () => {
  const handle = await fs.promises.open('tests/node/test/fixtures/x.txt', 'r');
  const target = Buffer.alloc(4);
  assert.deepStrictEqual(await handle.readFile({ buffer: target }), Buffer.from('xyz\n'));
  await handle.close();
})().then(() => undefined);
new Promise((resolve, reject) => {
  fs.readFile('tests/node/test/fixtures/x.txt', { buffer: Buffer.alloc(4) }, (error, value) => {
    if (error) reject(error);
    else {
      assert.deepStrictEqual(value, Buffer.from('xyz\n'));
      resolve();
    }
  });
}).then(() => undefined);
let size;
const factoryResult = fs.readFileSync('tests/node/test/fixtures/x.txt', {
  buffer(fileSize) {
    size = fileSize;
    return Buffer.alloc(fileSize + 2);
  }
});
assert.strictEqual(size, 4);
assert.deepStrictEqual(factoryResult, Buffer.from('xyz\n'));
const fsBinding = require('internal/test/binding').internalBinding('fs');
const originalFstat = fsBinding.fstat;
fsBinding.fstat = function (...args) {
  const result = Reflect.apply(originalFstat, this, args);
  return Promise.resolve(result).then((stats) => stats);
};
const stats = fs.lstatSync('tests/node/test/fixtures/x.txt');
for (const name of ['atime', 'mtime', 'ctime', 'birthtime']) {
  assert(stats[name] instanceof Date, name + ' should be a Date');
  assert(Number.isInteger(stats[name].getTime()), name + ' should expose an integral time');
}
const encodings = ['ascii', 'utf8', 'utf16le', 'ucs2', 'base64', 'binary', 'hex'];
for (const encoding of encodings) {
  const expected = pathBuffer.toString(encoding);
  assert.strictEqual(fs.realpathSync(canonical, { encoding }), expected, encoding + ' object');
  assert.strictEqual(fs.realpathSync(canonical, encoding), expected, encoding + ' string');
  assert.strictEqual(fs.realpathSync(pathBuffer, { encoding }), expected, encoding + ' buffer object');
  assert.strictEqual(fs.realpathSync(pathBuffer, encoding), expected, encoding + ' buffer string');
}
