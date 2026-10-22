'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

async function main() {
  const root = path.join(process.cwd(), `quench-link-${process.pid}`);
  fs.mkdirSync(root);
  const source = path.join(root, 'source');
  const syncLink = path.join(root, 'sync-link');
  const callbackLink = path.join(root, 'callback-link');
  const promiseLink = path.join(root, 'promise-link');
  fs.writeFileSync(source, 'linked');

  fs.linkSync(source, syncLink);
  assert.equal(fs.readFileSync(syncLink, 'utf8'), 'linked');
  await new Promise((resolve, reject) => {
    fs.link(source, callbackLink, (error) => error ? reject(error) : resolve());
  });
  await fs.promises.link(source, promiseLink);
  for (const linkedPath of [syncLink, callbackLink, promiseLink]) {
    assert.equal(fs.statSync(linkedPath).ino, fs.statSync(source).ino);
  }
  fs.rmSync(root, { recursive: true });
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
