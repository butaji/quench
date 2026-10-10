'use strict';

const assert = require('node:assert/strict');
const glob = require('glob');
const fs = require('node:fs');
const fsPromises = require('node:fs/promises');

assert.strictEqual(fsPromises, fs.promises);
assert.strictEqual(fsPromises.constants, fs.constants);
assert.strictEqual(typeof fs.constants.O_RDONLY, 'number');
assert.strictEqual(Object.getPrototypeOf(fs.constants), null);

const matches = glob.sync('test-*.cjs', { cwd: __dirname });
assert.ok(matches.includes('test-glob.cjs'));
assert.ok(matches.includes('test-semver.cjs'));

const asyncMatches = glob.glob('test-*.cjs', { cwd: __dirname });
Promise.all([asyncMatches, fsPromises.opendir(__dirname)]).then(async ([entries, dir]) => {
    assert.ok(entries.includes('test-glob.cjs'));
    assert.ok(entries.includes('test-semver.cjs'));
    let visited = 0;
    for await (const entry of dir) {
      assert.ok(entry instanceof fs.Dirent);
      assert.equal(entry.parentPath, __dirname);
      visited++;
      if (visited === 3) break;
    }
    assert.equal(visited, 3);

    const syncDir = fs.opendirSync(__dirname);
    assert.ok(syncDir instanceof fs.Dir);
    assert.ok(syncDir.readSync() instanceof fs.Dirent);
    syncDir.closeSync();

    const concurrentDir = await fsPromises.opendir(__dirname);
    const pendingRead = concurrentDir.read();
    assert.throws(() => concurrentDir.closeSync(), { code: 'ERR_DIR_CONCURRENT_OPERATION' });
    assert.throws(() => concurrentDir.readSync(), { code: 'ERR_DIR_CONCURRENT_OPERATION' });
    await pendingRead;
    concurrentDir.closeSync();

    const closedDir = await fsPromises.opendir(__dirname);
    await closedDir.close();
    await assert.rejects(closedDir.close(), { code: 'ERR_DIR_CLOSED' });
  })
  .catch((error) => {
    console.error(error.stack || error);
    process.exitCode = 1;
  });
