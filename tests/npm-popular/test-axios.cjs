'use strict';

const assert = require('node:assert/strict');
const axios = require('axios');

const adapter = async (config) => ({
  data: { method: config.method, url: config.url },
  status: 200,
  statusText: 'OK',
  headers: {},
  config,
});

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

