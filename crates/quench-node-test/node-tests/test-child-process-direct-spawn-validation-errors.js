'use strict';
const assert = require('node:assert/strict');
const { ChildProcess } = require('node:child_process');
const cases = [
  [undefined, 'ERR_INVALID_ARG_TYPE', 'The "options" argument must be of type object. Received undefined'],
  [null, 'ERR_INVALID_ARG_TYPE', 'The "options" argument must be of type object. Received null'],
  [[], 'ERR_INVALID_ARG_TYPE', 'The "options" argument must be of type object. Received an instance of Array'],
  [{file: 1, stdio: 'bad'}, 'ERR_INVALID_ARG_VALUE', "The argument 'stdio' is invalid. Received 'bad'"],
  [{file: '/bin/true', args: 1}, 'ERR_INVALID_ARG_TYPE', 'The "options.args" property must be an instance of Array. Received type number (1)'],
  [{file: '/bin/true', args: [], serialization: 'bad'}, 'ERR_INVALID_ARG_VALUE', "The property 'options.serialization' must be one of: undefined, 'json', 'advanced'. Received 'bad'"],
];
for (const [options, code, message] of cases) {
  const child = new ChildProcess();
  assert.throws(() => child.spawn(options), {name:'TypeError',code,message});
}
const child = new ChildProcess();
const events = [];
const file = '/quench-nonexistent-direct-spawn-oracle';
const argv = ['custom-argv0', 'argument'];
child.on('spawn', () => { throw new Error('unexpected spawn'); });
child.on('exit', () => { throw new Error('unexpected exit'); });
child.on('error', error => {
  assert.equal(error.code, 'ENOENT'); assert.equal(error.errno, -2);
  assert.equal(error.syscall, 'spawn ' + file); assert.equal(error.path, file);
  assert.deepEqual(error.spawnargs, ['argument']);
  assert.equal(child.exitCode, -2); assert.equal(child._handle, null);
  assert.equal(child.spawnargs, argv);
  events.push('error');
});
child.on('close', (code, signal) => {
  assert.equal(code, -2); assert.equal(signal, null);
  events.push('close');
  assert.deepEqual(events, ['returned', 'error', 'microtask', 'close']);
  console.log('CHILD_PROCESS_DIRECT_SPAWN_VALIDATION_ERROR_OK');
});
assert.equal(child.spawn({file, args: argv, stdio: 'pipe', envPairs: []}), -2);
events.push('returned');
queueMicrotask(() => events.push('microtask'));
