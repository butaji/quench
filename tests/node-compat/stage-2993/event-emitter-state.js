'use strict';

const assert = require('assert');
const EventEmitter = require('events');

const emitter = new EventEmitter();
emitter.setMaxListeners(1);
emitter.on(null, () => {});
emitter.on(null, () => {});
assert.strictEqual(emitter._events.null.warned, true);

const symbol = Symbol('stage');
emitter.on(symbol, () => {});
emitter.on(symbol, () => {});
assert.strictEqual(emitter._events[symbol].warned, true);

function Subclass() {
  this.on('stale', () => {});
  EventEmitter.call(this);
}
Object.setPrototypeOf(Subclass.prototype, EventEmitter.prototype);
const instance = new Subclass();
assert.deepStrictEqual(Object.keys(instance._events), []);

console.log('ok');
