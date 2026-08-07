const { EventEmitter } = require("events");
const emitter = new EventEmitter();
const listener = () => {};
emitter.on("ready", listener);
if (emitter._events.ready !== listener) {
  throw new Error("single listener was not stored directly");
}
if (emitter.listeners("ready")[0] !== listener) {
  throw new Error("listener identity was not returned");
}
