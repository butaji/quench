const { EventEmitter, on } = require("events");

(async () => {
  const emitter = new EventEmitter();
  const events = on(emitter, "data");

  queueMicrotask(() => {
    emitter.emit("data", "first");
    emitter.emit("data", "second", 2);
  });

  const first = await events.next();
  const second = await events.next();
  await events.return();
  const done = await events.next();

  if (first.value.join(",") !== "first") {
    throw new Error("first event mismatch");
  }
  if (second.value.join(",") !== "second,2") {
    throw new Error("second event mismatch");
  }
  if (!done.done) throw new Error("iterator should finish after return");
  if (emitter.listenerCount("data") !== 0) {
    throw new Error("iterator should remove its listener");
  }
})();
