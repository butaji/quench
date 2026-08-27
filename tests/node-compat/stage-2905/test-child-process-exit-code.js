'use strict';

const assert = require('node:assert');
const { spawn } = require('node:child_process');

const child = spawn(process.argv[0], ['exit.js', '23']);
child.on('exit', (code, signal) => {
  assert.strictEqual(code, 23);
  assert.strictEqual(signal, null);
});
