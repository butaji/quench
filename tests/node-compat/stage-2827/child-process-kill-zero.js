const assert = require('assert');
const child = require('child_process').spawn(process.execPath, [
  '-e',
  "process.stdout.write('x')"
]);

child.stdout.on('data', (value) => {
  assert.strictEqual(value.toString(), 'x');
  assert.strictEqual(child.kill(0), true);
  assert.strictEqual(child.stdin.write('x'), true);
  child.stdin.end();
});
