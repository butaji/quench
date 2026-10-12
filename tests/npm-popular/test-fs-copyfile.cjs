'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const prefix = path.join(process.cwd(), `quench-copyfile-${process.pid}`);
const source = `${prefix}-source`;
const syncTarget = `${prefix}-sync`;
const callbackTarget = `${prefix}-callback`;
const promiseTarget = `${prefix}-promise`;
fs.writeFileSync(source, 'copy data');

fs.copyFileSync(source, syncTarget);
assert.equal(fs.readFileSync(syncTarget, 'utf8'), 'copy data');
assert.throws(() => fs.copyFileSync(source, syncTarget, fs.constants.COPYFILE_EXCL), { code: 'EEXIST' });

const callbackResult = new Promise((resolve, reject) => {
  fs.copyFile(source, callbackTarget, (error) => error ? reject(error) : resolve());
});
Promise.all([
  callbackResult,
  fs.promises.copyFile(source, promiseTarget, fs.constants.COPYFILE_FICLONE),
]).then(() => {
  assert.equal(fs.readFileSync(callbackTarget, 'utf8'), 'copy data');
  assert.equal(fs.readFileSync(promiseTarget, 'utf8'), 'copy data');
  for (const file of [source, syncTarget, callbackTarget, promiseTarget]) fs.rmSync(file);
});
