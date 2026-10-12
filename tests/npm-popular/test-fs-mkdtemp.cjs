'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const prefix = path.join(process.cwd(), `quench-mkdtemp-${process.pid}-`);
const syncDirectory = fs.mkdtempSync(prefix);
assert.equal(syncDirectory.startsWith(prefix), true);
assert.match(syncDirectory.slice(prefix.length), /^[a-z0-9]{6}$/i);
assert.equal(fs.statSync(syncDirectory).isDirectory(), true);

const callbackResult = new Promise((resolve, reject) => {
  fs.mkdtemp(prefix, (error, directory) => {
    if (error) return reject(error);
    try {
      assert.equal(directory.startsWith(prefix), true);
      assert.equal(fs.statSync(directory).isDirectory(), true);
      resolve(directory);
    } catch (error) {
      reject(error);
    }
  });
});

Promise.all([callbackResult, fs.promises.mkdtemp(prefix)]).then((directories) => {
  for (const directory of [syncDirectory, ...directories]) {
    fs.rmSync(directory, { recursive: true });
  }
});
