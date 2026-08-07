const { EventEmitter } = require("events");
const emitter = new EventEmitter();
const listener = () => {};
emitter.on("ready", listener);
if (emitter.rawListeners("ready")[0] !== listener) {
  throw new Error("raw listener identity was not preserved");
}
if (!emitter.eventNames().includes("ready")) {
  throw new Error("single listener event name was omitted");
}
