const { EventEmitter, on } = require("events");

(async () => {
  const emitter = new EventEmitter();
  const controller = new AbortController();
  const events = on(emitter, "data", { signal: controller.signal });

  controller.abort();
  let error;
  try {
    await events.next();
  } catch (caught) {
    error = caught;
  }

  if (!error || error.name !== "AbortError") {
    throw new Error("events.on should reject with AbortError");
  }
  if (emitter.listenerCount("data") !== 0) {
    throw new Error("abort should remove the event listener");
  }
})();
