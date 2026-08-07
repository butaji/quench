const { EventEmitter } = require("events");
const receiver = {};
EventEmitter.prototype.on.call(receiver, "ready", () => {});
EventEmitter.prototype.on.call(receiver, "ready", () => {});
if (!Array.isArray(receiver._events.ready)) {
  throw new Error("generic EventEmitter receiver was not initialized");
}
