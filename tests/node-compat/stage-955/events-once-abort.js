const { EventEmitter, once } = require("events");

(async () => {
  const emitter = new EventEmitter();
  const controller = new AbortController();
  const result = once(emitter, "ready", { signal: controller.signal });

  controller.abort();
  let error;
  try {
    await result;
  } catch (caught) {
    error = caught;
  }
  if (!error || error.name !== "AbortError") {
    throw new Error("once should reject with AbortError");
  }
  if (emitter.listenerCount("ready") !== 0) {
    throw new Error("abort should remove the once listener");
  }
})();
