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

  const readBuffers = [Buffer.alloc(8), new Uint8Array(8)];
  const readFd = fs.openSync(syncPath, 'r');
  assert.equal(fs.readvSync(readFd, readBuffers, 0), 16);
  fs.closeSync(readFd);
  assert.equal(Buffer.concat(readBuffers).toString(), 'popular packages');

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

  const callbackReadFd = fs.openSync(callbackPath, 'r');
  const callbackReadBuffers = [Buffer.alloc(8), Buffer.alloc(8)];
  await new Promise((resolve, reject) => {
    fs.readv(callbackReadFd, callbackReadBuffers, (error, bytesRead, readBuffers) => {
      if (error) return reject(error);
      assert.equal(bytesRead, 16);
      assert.equal(readBuffers, callbackReadBuffers);
      resolve();
    });
  });
  fs.closeSync(callbackReadFd);
  assert.equal(Buffer.concat(callbackReadBuffers).toString(), 'popular packages');

  const handle = await fs.promises.open(path.join(root, 'promise'), 'w');
  const result = await handle.writev(buffers);
  assert.equal(result.bytesWritten, 16);
  assert.equal(result.buffers, buffers);
  await handle.close();

  const readHandle = await fs.promises.open(syncPath, 'r');
  const readResult = await readHandle.readv([Buffer.alloc(8), Buffer.alloc(8)]);
  assert.equal(readResult.bytesRead, 16);
  assert.equal(Buffer.concat(readResult.buffers).toString(), 'popular packages');
  await readHandle.close();
  fs.rmSync(root, { recursive: true });
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
