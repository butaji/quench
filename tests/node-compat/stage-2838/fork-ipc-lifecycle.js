const assert = require('assert');
const { fork } = require('child_process');

const child = fork('entry.js', ['child'], { silent: true });
const messages = [];
let output = '';
child.on('message', (value) => {
  messages.push(value);
  if (value === '2') child.disconnect();
});
child.stdout.on('data', (value) => { output += value; });
child.on('exit', () => {
  assert.deepStrictEqual(messages, ['1', '2']);
  assert.strictEqual(output, '3');
});
