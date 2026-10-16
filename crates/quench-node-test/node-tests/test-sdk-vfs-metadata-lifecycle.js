'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vfs = require('node:vfs').create({ emitExperimentalWarning: false });
vfs.writeFileSync('/file', 'metadata');
vfs.symlinkSync('/file', '/link');
const root = vfs.mount();
let fd;
try {
  assert.equal(fs.statSync(root + '/file').size, 8);
  assert.equal(fs.statSync(root + '/link').isFile(), true);
  assert.equal(fs.lstatSync(root + '/link').isSymbolicLink(), true);
  assert.equal(fs.statSync(root + '/missing', { throwIfNoEntry: false }), undefined);
  assert.equal(fs.lstatSync(root + '/missing', { throwIfNoEntry: false }), undefined);
  fd = fs.openSync(root + '/file', 'r+');
  assert.equal(fs.fstatSync(fd).size, 8);
  // This configured SDK calls h.fstatSync(fd) without forwarding options.
  assert.equal(typeof fs.fstatSync(fd, { bigint: true }).size, 'number');
  fs.fchmodSync(fd, '600');
  assert.equal(fs.fstatSync(fd).mode & 0o777, 0o600);
  fs.closeSync(fd); fd = undefined;
} finally {
  if (fd !== undefined) fs.closeSync(fd);
  vfs.unmount();
}
console.log('SDK virtual stat/lstat/fstat/fchmod/close lifecycle passed');
