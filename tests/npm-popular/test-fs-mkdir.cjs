'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const root = path.join(process.cwd(), `quench-mkdir-${process.pid}`);
const callbackTarget = path.join(root, 'callback', 'nested');
const promiseTarget = path.join(root, 'promise', 'nested');
const callbackRmdirTarget = path.join(root, 'callback-rmdir');
const promiseRmdirTarget = path.join(root, 'promise-rmdir');
fs.mkdirSync(root);
fs.mkdirSync(callbackRmdirTarget);
fs.mkdirSync(promiseRmdirTarget);
fs.rmdirSync(callbackRmdirTarget);

const callbackResult = new Promise((resolve, reject) => {
  fs.mkdir(callbackTarget, { recursive: true }, (error, createdPath) => {
    if (error) return reject(error);
    try {
      assert.equal(fs.statSync(callbackTarget).isDirectory(), true);
      assert.equal(createdPath, path.join(root, 'callback'));
      resolve();
    } catch (error) {
      reject(error);
    }
  });
});
const callbackRmdirResult = new Promise((resolve, reject) => {
  fs.mkdirSync(callbackRmdirTarget);
  fs.rmdir(callbackRmdirTarget, (error) => error ? reject(error) : resolve());
});

Promise.all([
  callbackResult,
  callbackRmdirResult,
  fs.promises.mkdir(promiseTarget, { recursive: true }).then((createdPath) => {
    assert.equal(fs.statSync(promiseTarget).isDirectory(), true);
    assert.equal(createdPath, path.join(root, 'promise'));
  }),
  fs.promises.rmdir(promiseRmdirTarget),
]).then(() => fs.rmSync(root, { recursive: true }));
