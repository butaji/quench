'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');

const fields = ['type', 'bsize', 'frsize', 'blocks', 'bfree', 'bavail', 'files', 'ffree'];
const numeric = fs.statfsSync(process.cwd());
for (const field of fields) assert.equal(typeof numeric[field], 'number');

async function main() {
  const callbackStats = await new Promise((resolve, reject) => {
    fs.statfs(process.cwd(), { bigint: true }, (error, stats) => error ? reject(error) : resolve(stats));
  });
  const promiseStats = await fs.promises.statfs(process.cwd(), { bigint: true });
  for (const stats of [callbackStats, promiseStats]) {
    for (const field of fields) assert.equal(typeof stats[field], 'bigint');
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
