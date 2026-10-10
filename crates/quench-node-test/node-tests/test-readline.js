// readline — createInterface emits 'line' for each input line, then 'close'.
'use strict';
const assert = require('assert');
const { createInterface } = require('node:readline');

const got = [];
const rl = createInterface({ input: ['alpha', 'beta', 'gamma'] });
rl.on('line', (line) => got.push(line));
rl.on('close', () => {
  assert.deepStrictEqual(got, ['alpha', 'beta', 'gamma'], 'lines emitted in order');
  console.log('readline: ok');
});