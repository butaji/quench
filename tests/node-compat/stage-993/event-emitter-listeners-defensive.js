const { EventEmitter } = require("events");
const emitter = new EventEmitter();
emitter._events = undefined;
if (emitter.listeners("ready").length !== 0) {
  throw new Error("listeners did not handle missing event storage");
}
if (emitter.listeners().length !== 0) {
  throw new Error("listeners without an event was not empty");
}
