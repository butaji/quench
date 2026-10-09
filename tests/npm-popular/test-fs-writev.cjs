'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

async function main() {
  const root = path.join(process.cwd(), `quench-writev-${process.pid}`);
  fs.mkdirSync(root);
  const syncPath = path.join(root, 'sync');
  const buffers = [Buffer.from('popular '), Uint8Array.from(Buffer.from('packages'))];
  const fd = fs.openSync(syncPath, 'w');
  assert.equal(fs.writevSync(fd, buffers), 16);
  fs.closeSync(fd);
  assert.equal(fs.readFileSync(syncPath, 'utf8'), 'popular packages');

  const callbackPath = path.join(root, 'callback');
  const callbackFd = fs.openSync(callbackPath, 'w');
  await new Promise((resolve, reject) => {
    fs.writev(callbackFd, buffers, null, (error, bytesWritten, writtenBuffers) => {
      if (error) return reject(error);
      assert.equal(bytesWritten, 16);
      assert.equal(writtenBuffers, buffers);
      resolve();
    });
  });
  fs.closeSync(callbackFd);
  assert.equal(fs.readFileSync(callbackPath, 'utf8'), 'popular packages');

  const handle = await fs.promises.open(path.join(root, 'promise'), 'w');
  const result = await handle.writev(buffers);
  assert.equal(result.bytesWritten, 16);
  assert.equal(result.buffers, buffers);
  await handle.close();
  fs.rmSync(root, { recursive: true });
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
