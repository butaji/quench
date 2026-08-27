const assert = require('assert');
const { fork } = require('child_process');

const child = fork('child.js');
assert.throws(() => child.send(), { code: 'ERR_MISSING_ARGS' });
assert.throws(() => child.send(undefined), { code: 'ERR_MISSING_ARGS' });
assert.throws(() => child.send(Symbol()), { code: 'ERR_INVALID_ARG_TYPE' });
assert.strictEqual(child.send({ hello: 'world' }), true);
child.on('message', (value) => assert.strictEqual(value.foo, true));
