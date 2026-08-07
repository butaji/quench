const { EventEmitter } = require("events");

const emitter = new EventEmitter();
const listener = () => {};

emitter.on("ready", listener);
emitter.on("ready", listener);
emitter.on("other", listener);

if (emitter.listenerCount("ready") !== 2) {
  throw new Error("listenerCount should count listeners for one event");
}
if (emitter.listenerCount("other") !== 1) {
  throw new Error("listenerCount should isolate event names");
}
if (emitter.listenerCount("missing") !== 0) {
  throw new Error("listenerCount should return zero for missing events");
}
