'use strict';
const assert = require('assert');
const { exec } = require('child_process');
const { execFile } = require('child_process');
exec(`"${process.execPath}" -p 42`, (error, stdout, stderr) => {
  assert.strictEqual(error, null);
  assert.strictEqual(stdout, '42\n');
  assert.strictEqual(stderr, '');
});
execFile(process.execPath, ['-p', '42'], (error, stdout, stderr) => {
  assert.strictEqual(error, null);
  assert.strictEqual(stdout, '42\n');
  assert.strictEqual(stderr, '');
});
