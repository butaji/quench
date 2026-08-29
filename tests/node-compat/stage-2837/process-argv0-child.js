const assert = require('assert');
const cp = require('child_process');

const child = cp.spawn(process.execPath, ['entry.js', 'child']);
let output = '';
child.stdout.on('data', (value) => { output += value; });
child.stdout.on('end', () => assert.strictEqual(output, process.execPath));
