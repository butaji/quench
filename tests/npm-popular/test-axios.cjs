'use strict';

const assert = require('node:assert/strict');
const axios = require('axios');
const fs = require('node:fs');

const sourceText = fs.readFileSync(__filename, 'utf8');
const sourceBase64 = fs.readFileSync(__filename, { encoding: 'base64' });
assert.strictEqual(Buffer.from(sourceBase64, 'base64').toString('utf8'), sourceText);

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

