"use strict";

const assert = require("assert");
const eventsApi = require("node:events");

const emitter = new eventsApi.EventEmitter();
assert.strictEqual(
  Array.isArray(eventsApi.getEventListeners(emitter, "event")),
  true,
);
assert.strictEqual(typeof eventsApi.listenerCount(emitter, "event"), "number");
eventsApi.setMaxListeners(20, emitter);
assert.strictEqual(typeof eventsApi.getMaxListeners(emitter), "number");

console.log("events listener inspection passed");
