const { EventEmitter, once } = require("events");

(async () => {
  const emitter = new EventEmitter();
  const failure = new Error("boom");
  const result = once(emitter, "ready");

  if (emitter.listenerCount("ready") !== 1) {
    throw new Error("once should install the event listener");
  }
  emitter.emit("error", failure);

  let caught;
  try {
    await result;
  } catch (error) {
    caught = error;
  }
  if (caught !== failure) throw new Error("once should reject with the error");
  if (emitter.listenerCount("ready") !== 0) {
    throw new Error("error rejection should remove the event listener");
  }
})();
