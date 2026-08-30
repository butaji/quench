'use strict';

const assert = require('assert');
const { EventEmitter } = require('events');

const emitter = new EventEmitter({ captureRejections: true });
let uncaught = 0;
process.removeAllListeners('uncaughtException');
process.once('uncaughtException', () => { uncaught++; });
emitter.on('value', async () => { throw new Error('value'); });
emitter.once('error', () => { throw new Error('error'); });
emitter.emit('value');

setImmediate(() => {
  assert.strictEqual(uncaught, 1);
  console.log('ok');
});
