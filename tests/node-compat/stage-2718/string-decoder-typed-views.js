'use strict';

const assert = require('assert');
const { StringDecoder } = require('string_decoder');

assert.strictEqual(typeof Float16Array, 'function');
const source = Buffer.from('String for ArrayBufferView tests\n'.repeat(2));
const decoder = new StringDecoder('utf8');
for (const view of [
  new Int8Array(source.buffer, source.byteOffset, source.byteLength),
  new Uint16Array(source.buffer, source.byteOffset, source.byteLength / 2),
  new Float16Array(source.buffer, source.byteOffset, source.byteLength / 2),
  new DataView(source.buffer, source.byteOffset, source.byteLength),
]) {
  assert.strictEqual(decoder.write(view), source.toString('utf8'));
  assert.strictEqual(decoder.end(), '');
}
