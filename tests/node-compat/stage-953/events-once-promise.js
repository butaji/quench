const { EventEmitter, once } = require("events");

(async () => {
  const emitter = new EventEmitter();
  const result = once(emitter, "ready");

  if (emitter.listenerCount("ready") !== 1) {
    throw new Error("once should install one listener");
  }
  emitter.emit("ready", "value", 42);

  const values = await result;
  if (values.length !== 2 || values[0] !== "value" || values[1] !== 42) {
    throw new Error("once should resolve with all event arguments");
  }
  if (emitter.listenerCount("ready") !== 0) {
    throw new Error("once should remove its listener after emission");
  }
})();
