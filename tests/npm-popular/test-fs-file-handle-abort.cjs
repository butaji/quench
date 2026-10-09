'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
(async () => {
  const file = path.join(process.cwd(), `quench-filehandle-abort-${process.pid}`);
  const handle = await fs.promises.open(file, 'w+');
  const controller = new AbortController();
  process.nextTick(() => controller.abort());
  try {
    await assert.rejects(handle.writeFile(Buffer.alloc(6 * 1024 * 1024), { signal: controller.signal }), { name: 'AbortError' });
    await handle.sync();
    await handle.datasync();
  } finally {
    await handle.close();
    assert.equal(handle.fd, -1);
    const otherHandle = await fs.promises.open(__filename, 'r');
    try {
      await assert.rejects(handle.stat(), { code: 'EBADF', syscall: 'fstat' });
    } finally {
      await otherHandle.close();
    }
    await handle.close();
    fs.rmSync(file, { force: true });
  }
  const reader = await fs.promises.open(__filename, 'r');
  try {
    const direct = await reader.readFile('utf8');
    assert.ok(direct.includes('readFile'));
  } finally {
    await reader.close();
  }
  const promiseReader = await fs.promises.open(__filename, 'r');
  try {
    const viaPromises = await fs.promises.readFile(promiseReader, 'utf8');
    assert.ok(viaPromises.includes('readFile'));
  } finally {
    await promiseReader.close();
  }
})().catch((error) => { console.error(error); process.exitCode = 1; });
