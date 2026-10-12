'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');

async function main() {
  const expected = fs.realpathSync(__filename);
  const buffer = fs.realpathSync(__filename, { encoding: 'buffer' });
  const expectedBuffer = Buffer.from(expected);
  assert.equal(buffer.toString(), expectedBuffer.toString());
  assert.equal(fs.realpathSync(__filename, 'hex'), Buffer.from(expected).toString('hex'));

  const callbackPath = await new Promise((resolve, reject) => {
    fs.realpath(__filename, 'base64', (error, result) => error ? reject(error) : resolve(result));
  });
  assert.equal(callbackPath, Buffer.from(expected).toString('base64'));

  const promisePath = await fs.promises.realpath(__filename, { encoding: 'buffer' });
  assert.equal(promisePath.toString(), expected);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
