"use strict";

const assert = require("assert");
const eventsApi = require("node:events");

for (
  const name of [
    "EventEmitter",
    "EventEmitterAsyncResource",
    "addAbortListener",
    "once",
    "on",
    "getEventListeners",
    "getMaxListeners",
    "setMaxListeners",
    "listenerCount",
  ]
) {
  assert.strictEqual(typeof eventsApi[name], "function");
}
const emitter = new eventsApi.EventEmitter();
assert.strictEqual(emitter instanceof eventsApi.EventEmitter, true);
assert.strictEqual(eventsApi.getMaxListeners(emitter), 10);

console.log("events core api passed");
