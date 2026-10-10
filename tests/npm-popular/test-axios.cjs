'use strict';

const assert = require('node:assert/strict');
const axios = require('axios');
const { constants, isAscii, isUtf8, kMaxLength, kStringMaxLength } = require('node:buffer');
const fs = require('node:fs');

assert.notDeepStrictEqual({ package: 'axios' }, { package: 'other' });
assert.strictEqual(constants.MAX_LENGTH, kMaxLength);
assert.strictEqual(constants.MAX_STRING_LENGTH, kStringMaxLength);
const sourceText = fs.readFileSync(__filename, 'utf8');
const sourceBase64 = fs.readFileSync(__filename, { encoding: 'base64' });
assert.strictEqual(Buffer.from(sourceBase64, 'base64').toString('utf8'), sourceText);
assert.strictEqual(
  Buffer.concat([Buffer.from('axios '), Buffer.from('compat')]).toString(),
  'axios compat',
);
assert.strictEqual(
  Buffer.concat([Uint8Array.of(1, 2), Uint8Array.of(3)], 5).toString('hex'),
  '0102030000',
);
assert.strictEqual(
  JSON.stringify(Buffer.from('axios')),
  '{"type":"Buffer","data":[97,120,105,111,115]}',
);
assert.strictEqual(
  Buffer.from(JSON.parse(JSON.stringify(Buffer.from('axios')))).toString(),
  'axios',
);
const numericBuffer = Buffer.alloc(8);
assert.strictEqual(numericBuffer.writeInt32LE(-42, 0), 4);
assert.strictEqual(numericBuffer.readInt32LE(0), -42);
assert.strictEqual(Buffer.prototype.readUInt16LE, Buffer.prototype.readUint16LE);
assert.strictEqual(Buffer.compare(Buffer.from('a'), Buffer.from('b')), -1);
const copiedBuffer = Buffer.alloc(3);
assert.strictEqual(Buffer.from('axios').copy(copiedBuffer, 0, 1, 4), 3);
assert.strictEqual(copiedBuffer.toString(), 'xio');
assert.strictEqual(Buffer.alloc(3, 'C').toString(), 'CCC');
const encodedBuffer = Buffer.alloc(4);
assert.strictEqual(encodedBuffer.write('😊', 0, 3, 'utf8'), 0);
assert.strictEqual(encodedBuffer.write('😊', 0, 4, 'utf8'), 4);
assert.strictEqual(encodedBuffer.toString(), '😊');
assert.strictEqual(isAscii(Buffer.from('axios')), true);
assert.strictEqual(isAscii(Buffer.from('mañana')), false);
assert.strictEqual(isUtf8(Buffer.from('mañana')), true);

const adapter = async (config) => ({
  data: { method: config.method, url: config.url },
  status: 200,
  statusText: 'OK',
  headers: {},
  config,
});

const search = new URLSearchParams({ q: 'node compatibility', page: 2 });
assert.strictEqual(
  axios.getUri({ url: 'https://example.test/search', params: search }),
  'https://example.test/search?q=node+compatibility&page=2',
);
assert.strictEqual(URL.canParse('https://example.test/search'), true);
assert.strictEqual(URL.canParse('/search', 'https://example.test'), true);
assert.strictEqual(URL.canParse('/search'), false);
assert.throws(() => URL.canParse(), { code: 'ERR_MISSING_ARGS' });

const controller = new AbortController();
let abortEvent;
controller.signal.addEventListener('abort', (event) => { abortEvent = event; });
controller.abort();
assert.strictEqual(abortEvent.isTrusted, true);
assert.strictEqual(controller.signal.reason.name, 'AbortError');
assert.strictEqual(controller.signal.reason.code, 20);

axios.get('https://example.test/items', { adapter })
  .then((response) => {
    assert.strictEqual(response.status, 200);
    assert.deepStrictEqual(response.data, {
      method: 'get',
      url: 'https://example.test/items',
    });
  })
  .catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
