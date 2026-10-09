'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { once } = require('node:events');

async function main() {
  const root = path.join(process.cwd(), `quench-write-stream-${process.pid}`);
  fs.mkdirSync(root);
  const file = path.join(root, 'output.txt');
  const stream = fs.createWriteStream(file, { encoding: 'utf8' });
  stream.end('popular packages use file streams');
  await once(stream, 'close');
  assert.equal(fs.readFileSync(file, 'utf8'), 'popular packages use file streams');
  for (const options of [0, 1, true, false]) {
    assert.throws(() => fs.createWriteStream(file, options), {
      code: 'ERR_INVALID_ARG_TYPE',
      name: 'TypeError',
    });
  }
  fs.rmSync(root, { recursive: true });
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
