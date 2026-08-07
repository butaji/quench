const { EventEmitter } = require("events");
const emitter = new EventEmitter();
const removed = [];
const listener = () => {};
emitter.on("removeListener", (name) => removed.push(name));
emitter.on("first", listener);
emitter.on("second", listener);
if (emitter.removeAllListeners() !== emitter) {
  throw new Error("removeAllListeners was not chainable");
}
if (emitter.listeners("first").length || emitter.listeners("second").length) {
  throw new Error("listeners remained after removeAllListeners");
}
if (!removed.includes("first") || !removed.includes("second")) {
  throw new Error("remove notifications were missing");
}
