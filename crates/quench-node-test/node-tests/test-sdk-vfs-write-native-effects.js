'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'quench-sdk-write-'));
const file = path.join(root, 'data');
try {
  fs.writeFileSync(file, 'abc', { flag: fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_TRUNC, mode: 0o600 });
  assert.equal(fs.readFileSync(file, 'utf8'), 'abc');
  if (process.platform !== 'win32') assert.equal(fs.statSync(file).mode & 0o777, 0o600);
  const fd = fs.openSync(file, 'w+');
  try {
    fs.writeSync(fd, Buffer.from('00'));
    fs.writeFileSync(fd, Buffer.from('AB'), { flush: true });
    fs.writeFileSync(fd, 'C');
    fs.appendFileSync(fd, 'D', { flag: 'w' });
    assert.equal(fs.readFileSync(file, 'utf8'), '00ABCD');
    assert.equal(fs.fstatSync(fd).size, 6);
  } finally { fs.closeSync(fd); }
  const from = Buffer.from;
  let conversions = 0;
  Buffer.from = function(data, ...args) { if (data === '4142') conversions++; return from.call(this, data, ...args); };
  try { fs.writeFileSync(file, '4142', { encoding: 'hex' }); }
  finally { Buffer.from = from; }
  assert.equal(conversions, 1);
  assert.equal(fs.readFileSync(file, 'utf8'), 'AB');
  const raw = Uint8Array.from([9, 8, 7, 6]);
  fs.writeFileSync(file, new DataView(raw.buffer, 1, 2));
  assert.deepEqual(fs.readFileSync(file), Buffer.from([8, 7]));
  assert.throws(() => fs.writeFileSync(999999, 'x'), error => error.code === 'EBADF' && error.syscall === 'write' && !Object.hasOwn(error, 'path'));
} finally { fs.rmSync(root, { recursive: true, force: true }); }
console.log('SDK native write flags/mode/bytes/fd ownership/position/errors passed');
