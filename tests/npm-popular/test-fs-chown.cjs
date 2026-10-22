'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

async function main() {
  const directory = fs.mkdtempSync(path.join(process.cwd(), 'quench-chown-'));
  const uid = typeof process.getuid === 'function' ? process.getuid() : 0;
  const gid = typeof process.getgid === 'function' ? process.getgid() : 0;
  const syncTarget = path.join(directory, 'sync');
  const callbackTarget = path.join(directory, 'callback');
  const promiseTarget = path.join(directory, 'promise');
  const descriptorTarget = path.join(directory, 'descriptor');
  try {
    for (const target of [syncTarget, callbackTarget, promiseTarget, descriptorTarget]) {
      fs.writeFileSync(target, 'owner');
    }

    assert.equal(fs.chownSync(syncTarget, uid, gid), undefined);
    await new Promise((resolve, reject) => {
      fs.chown(callbackTarget, uid, gid, (error) => error ? reject(error) : resolve());
    });
    await fs.promises.chown(promiseTarget, uid, gid);

    const descriptor = fs.openSync(descriptorTarget, 'r');
    try {
      assert.equal(fs.fchownSync(descriptor, uid, gid), undefined);
      await new Promise((resolve, reject) => {
        fs.fchown(descriptor, uid, gid, (error) => error ? reject(error) : resolve());
      });
    } finally {
      fs.closeSync(descriptor);
    }

    const symlink = path.join(directory, 'link');
    fs.symlinkSync(syncTarget, symlink);
    assert.equal(fs.lchownSync(symlink, uid, gid), undefined);
    await new Promise((resolve, reject) => {
      fs.lchown(symlink, uid, gid, (error) => error ? reject(error) : resolve());
    });
    await fs.promises.lchown(symlink, uid, gid);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
