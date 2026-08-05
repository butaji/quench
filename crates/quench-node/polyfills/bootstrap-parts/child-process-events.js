const __quenchChildEventOrderRequire = globalThis.require;
const __quenchChildEventOrder = __quenchChildEventOrderRequire("child_process");
const __quenchEventOrderSpawn = __quenchChildEventOrder.spawn;
__quenchChildEventOrder.spawn = (...args) => {
  const child = __quenchEventOrderSpawn(...args);
  child.on = globalThis.__nodeEventEmitter.prototype.on;
  child.emit = (
    (emit) =>
    (event, ...values) => {
      if (event === "exit" && !child.__spawnEmitted) {
        child.__spawnEmitted = true;
        emit.call(child, "spawn");
      }
      return emit.call(child, event, ...values);
    }
  )(child.emit);
  return child;
};
