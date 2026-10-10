'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const prefix = path.join(process.cwd(), `quench-symlink-${process.pid}`);
const source = `${prefix}-source`;
const syncLink = `${prefix}-sync`;
const callbackLink = `${prefix}-callback`;
const promiseLink = `${prefix}-promise`;
fs.writeFileSync(source, 'linked data');

fs.symlinkSync(source, syncLink, 'file');
assert.equal(fs.readlinkSync(syncLink), source);
assert.equal(fs.readFileSync(syncLink, 'utf8'), 'linked data');
assert.equal(fs.lstatSync(syncLink).isSymbolicLink(), true);

const callbackResult = new Promise((resolve, reject) => {
  fs.symlink(source, callbackLink, 'file', (error) => error ? reject(error) : resolve());
});
const callbackMetadata = new Promise((resolve, reject) => {
  fs.lstat(syncLink, (error, stats) => {
    if (error) return reject(error);
    try {
      assert.equal(stats.isSymbolicLink(), true);
      resolve();
    } catch (error) {
      reject(error);
    }
  });
});
const callbackReadlink = new Promise((resolve, reject) => {
  fs.readlink(syncLink, (error, target) => {
    if (error) return reject(error);
    try {
      assert.equal(target, source);
      resolve();
    } catch (error) {
      reject(error);
    }
  });
});
Promise.all([
  callbackResult,
  callbackMetadata,
  callbackReadlink,
  fs.promises.symlink(source, promiseLink, 'file'),
  fs.promises.lstat(syncLink).then((stats) => assert.equal(stats.isSymbolicLink(), true)),
  fs.promises.readlink(syncLink).then((target) => assert.equal(target, source)),
]).then(() => {
  assert.equal(fs.readFileSync(callbackLink, 'utf8'), 'linked data');
  assert.equal(fs.readFileSync(promiseLink, 'utf8'), 'linked data');
  for (const entry of [source, syncLink, callbackLink, promiseLink]) fs.rmSync(entry);
});
