'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const root = path.join(process.cwd(), `quench-cp-${process.pid}`);
const source = path.join(root, 'source');
fs.mkdirSync(path.join(source, 'nested'), { recursive: true });
fs.writeFileSync(path.join(source, 'root.txt'), 'root');
fs.writeFileSync(path.join(source, 'nested', 'child.txt'), 'child');

const syncTarget = path.join(root, 'sync');
fs.cpSync(source, syncTarget, { recursive: true });
assert.equal(fs.readFileSync(path.join(syncTarget, 'nested', 'child.txt'), 'utf8'), 'child');

const callbackTarget = path.join(root, 'callback');
const callbackResult = new Promise((resolve, reject) => {
  fs.cp(source, callbackTarget, { recursive: true }, (error) => error ? reject(error) : resolve());
});
const promiseTarget = path.join(root, 'promise');
Promise.all([
  callbackResult,
  fs.promises.cp(source, promiseTarget, { recursive: true }),
]).then(() => {
  assert.equal(fs.readFileSync(path.join(callbackTarget, 'root.txt'), 'utf8'), 'root');
  assert.equal(fs.readFileSync(path.join(promiseTarget, 'nested', 'child.txt'), 'utf8'), 'child');
  fs.rmSync(root, { recursive: true });
});
