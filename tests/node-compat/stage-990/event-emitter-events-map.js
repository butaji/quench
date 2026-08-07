const { EventEmitter } = require("events");
const emitter = new EventEmitter();
if (emitter._events instanceof Object) {
  throw new Error("EventEmitter events map has a prototype");
}
if (Object.keys(emitter._events).length !== 0) {
  throw new Error("EventEmitter events map was not empty");
}
emitter.setMaxListeners(5);
if (Object.keys(emitter._events).length !== 0) {
  throw new Error("setMaxListeners mutated event entries");
}
