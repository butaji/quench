'use strict';

const assert = require('node:assert/strict');
const dotenv = require('dotenv');

assert.deepStrictEqual(dotenv.parse(Buffer.from('PORT=3000\nNAME="quench runtime"\n')), {
  PORT: '3000',
  NAME: 'quench runtime',
});
