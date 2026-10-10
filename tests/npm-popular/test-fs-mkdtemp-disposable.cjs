'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

async function main() {
  const sync = fs.mkdtempDisposableSync(path.join(process.cwd(), 'quench-disposable-sync-'));
  assert.equal(fs.existsSync(sync.path), true);
  sync[Symbol.dispose]();
  assert.equal(fs.existsSync(sync.path), false);
  sync.remove();

  const asyncDisposable = await fs.promises.mkdtempDisposable(
    path.join(process.cwd(), 'quench-disposable-async-'),
  );
  assert.equal(fs.existsSync(asyncDisposable.path), true);
  await asyncDisposable[Symbol.asyncDispose]();
  assert.equal(fs.existsSync(asyncDisposable.path), false);
  await asyncDisposable.remove();
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
