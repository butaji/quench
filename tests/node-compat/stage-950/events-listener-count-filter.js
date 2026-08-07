const { EventEmitter } = require("events");

const emitter = new EventEmitter();
const first = () => {};
const second = () => {};

emitter.on("ready", first);
emitter.on("ready", second);
emitter.on("ready", first);

if (emitter.listenerCount("ready") !== 3) {
  throw new Error("listenerCount should count all listeners");
}
if (emitter.listenerCount("ready", first) !== 2) {
  throw new Error("listenerCount should filter by listener identity");
}
if (emitter.listenerCount("ready", second) !== 1) {
  throw new Error("listenerCount should count one matching listener");
}
