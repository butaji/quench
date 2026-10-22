'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

async function main() {
  const directory = fs.mkdtempSync(path.join(process.cwd(), 'quench-truncate-'));
  const pathTarget = path.join(directory, 'path');
  const callbackTarget = path.join(directory, 'callback');
  const promiseTarget = path.join(directory, 'promise');
  const descriptorTarget = path.join(directory, 'descriptor');
  try {
    fs.writeFileSync(pathTarget, 'abcdefgh');
    fs.truncateSync(pathTarget, 4);
    assert.equal(fs.readFileSync(pathTarget, 'utf8'), 'abcd');

    fs.writeFileSync(callbackTarget, 'abcdefgh');
    await new Promise((resolve, reject) => {
      fs.truncate(callbackTarget, 3, (error) => error ? reject(error) : resolve());
    });
    assert.equal(fs.readFileSync(callbackTarget, 'utf8'), 'abc');

    fs.writeFileSync(promiseTarget, 'abcdefgh');
    await fs.promises.truncate(promiseTarget, 2);
    assert.equal(fs.readFileSync(promiseTarget, 'utf8'), 'ab');

    fs.writeFileSync(descriptorTarget, 'abcdefgh');
    const descriptor = fs.openSync(descriptorTarget, 'r+');
    try {
      fs.ftruncateSync(descriptor, 5);
      await new Promise((resolve, reject) => {
        fs.ftruncate(descriptor, 2, (error) => error ? reject(error) : resolve());
      });
    } finally {
      fs.closeSync(descriptor);
    }
    assert.equal(fs.readFileSync(descriptorTarget, 'utf8'), 'ab');
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
