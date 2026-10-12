'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const prefix = path.join(process.cwd(), `quench-rename-${process.pid}`);
const syncSource = `${prefix}-sync-source`;
const syncTarget = `${prefix}-sync-target`;
fs.writeFileSync(syncSource, 'rename data');
assert.equal(fs.renameSync(syncSource, syncTarget), undefined);
assert.equal(fs.readFileSync(syncTarget, 'utf8'), 'rename data');

const callbackSource = `${prefix}-callback-source`;
const callbackTarget = `${prefix}-callback-target`;
fs.writeFileSync(callbackSource, 'callback');
const callbackResult = new Promise((resolve, reject) => {
  fs.rename(callbackSource, callbackTarget, (error) => error ? reject(error) : resolve());
});
const promiseSource = `${prefix}-promise-source`;
const promiseTarget = `${prefix}-promise-target`;
fs.writeFileSync(promiseSource, 'promise');
Promise.all([
  callbackResult,
  fs.promises.rename(promiseSource, promiseTarget),
]).then(() => {
  assert.equal(fs.readFileSync(callbackTarget, 'utf8'), 'callback');
  assert.equal(fs.readFileSync(promiseTarget, 'utf8'), 'promise');
  for (const file of [syncTarget, callbackTarget, promiseTarget]) fs.rmSync(file);
});
