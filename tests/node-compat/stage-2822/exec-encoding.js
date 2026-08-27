const assert = require('assert');
const { exec } = require('child_process');

const command = 'node script.js child';
exec(command, {}, (error, stdout, stderr) => {
  assert.ifError(error);
  assert.strictEqual(typeof stdout, 'string');
  assert.strictEqual(stdout, 'foo\n');
  assert.strictEqual(stderr, 'bar\n');
});

exec(command, { encoding: 'buffer' }, (error, stdout, stderr) => {
  assert.ifError(error);
  assert(stdout instanceof Buffer);
  assert.strictEqual(stdout.toString(), 'foo\n');
  assert.strictEqual(stderr.toString(), 'bar\n');
});
