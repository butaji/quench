'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const root = path.join(process.cwd(), `quench-mkdir-${process.pid}`);
const callbackTarget = path.join(root, 'callback', 'nested');
const promiseTarget = path.join(root, 'promise', 'nested');

const callbackResult = new Promise((resolve, reject) => {
  fs.mkdir(callbackTarget, { recursive: true }, (error, createdPath) => {
    if (error) return reject(error);
    try {
      assert.equal(fs.statSync(callbackTarget).isDirectory(), true);
      assert.equal(createdPath, root);
      resolve();
    } catch (error) {
      reject(error);
    }
  });
});

Promise.all([
  callbackResult,
  fs.promises.mkdir(promiseTarget, { recursive: true }).then((createdPath) => {
    assert.equal(fs.statSync(promiseTarget).isDirectory(), true);
    assert.equal(createdPath, path.join(root, 'promise'));
  }),
]).then(() => fs.rmSync(root, { recursive: true }));
