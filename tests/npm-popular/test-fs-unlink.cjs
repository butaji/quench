'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const prefix = path.join(process.cwd(), `quench-unlink-${process.pid}`);
const syncTarget = `${prefix}-sync`;
const callbackTarget = `${prefix}-callback`;
const promiseTarget = `${prefix}-promise`;
for (const target of [syncTarget, callbackTarget, promiseTarget]) fs.writeFileSync(target, target);

assert.equal(fs.unlinkSync(syncTarget), undefined);
assert.equal(fs.existsSync(syncTarget), false);
const callbackResult = new Promise((resolve, reject) => {
  fs.unlink(callbackTarget, (error) => error ? reject(error) : resolve());
});
Promise.all([callbackResult, fs.promises.unlink(promiseTarget)]).then(() => {
  assert.equal(fs.existsSync(callbackTarget), false);
  assert.equal(fs.existsSync(promiseTarget), false);
});
