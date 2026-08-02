const assert = require('assert');
const events = require('node:events');
const emitter = new events.EventEmitter();
events.once(emitter, 'ready').then((args) => assert.deepStrictEqual(args, ['ok']));
emitter.emit('ready', 'ok');
