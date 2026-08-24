'use strict';

const assert = require('assert');
const fs = require('fs');

const path = '/tmp/quench-node-stage-2637';
const source = Buffer.from('typed-array write'.repeat(8));
const view = new Int16Array(source.buffer, source.byteOffset, source.byteLength / 2);

fs.writeFileSync(path, view);
assert.strictEqual(fs.readFileSync(path, 'utf8'), source.toString());

fs.writeFile(path, new DataView(source.buffer, source.byteOffset, source.byteLength), (error) => {
  assert.strictEqual(error, null);
  assert.strictEqual(fs.readFileSync(path, 'utf8'), source.toString());
  fs.unlinkSync(path);
  console.log('PASS fs typed-array write');
});
