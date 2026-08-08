globalThis.__nodeTimersPromises = {
  setTimeout: (_delay = 0, value, options = {}) =>
    new Promise((resolve, reject) =>
      queueMicrotask(() => {
        if (options && options.signal && options.signal.aborted) {
          const error = new Error("The operation was aborted");
          error.name = "AbortError";
          error.code = "ABORT_ERR";
          reject(error);
          return;
        }
        if (Number(_delay) > 0) {
          globalThis.__quench_sleep_ms(Math.max(0, Number(_delay)));
        }
        resolve(value);
      })
    ),
  setImmediate: (value, options = {}) =>
    new Promise((resolve, reject) =>
      queueMicrotask(() => {
        if (options && options.signal && options.signal.aborted) {
          const error = new Error("The operation was aborted");
          error.name = "AbortError";
          error.code = "ABORT_ERR";
          reject(error);
          return;
        }
        resolve(value);
      })
    ),
  setInterval: async function* (_delay = 0, value, options = {}) {
    while (true) {
      if (options && options.signal && options.signal.aborted) {
        const error = new Error("The operation was aborted");
        error.name = "AbortError";
        error.code = "ABORT_ERR";
        throw error;
      }
      if (Number(_delay) > 0) {
        globalThis.__quench_sleep_ms(Math.max(0, Number(_delay)));
      } else await new Promise((resolve) => queueMicrotask(resolve));
      yield value;
    }
  }
};

const processListeners = {};
process.stdout ||= { isTTY: false };
process.stderr ||= { isTTY: false };
process.on = (event, listener) => {
  (processListeners[event] ||= []).push(listener);
  return process;
};
process.once = (event, listener) => {
  const once = (...args) => {
    process.removeListener(event, once);
    listener(...args);
  };
  return process.on(event, once);
};
process.removeListener = (event, listener) => {
  processListeners[event] = (processListeners[event] || []).filter(
    (item) => item !== listener
  );
  return process;
};
process.removeAllListeners = (event) => {
  if (event) delete processListeners[event];
  else {
    Object.keys(processListeners).forEach(
      (key) => delete processListeners[key]
    );
  }
};
process.emit = (event, ...args) => {
  const listeners = processListeners[event] || [];
  listeners.forEach((listener) => listener(...args));
  return listeners.length > 0;
};
process.emitWarning = (warning, options = {}) => {
  const message = warning instanceof Error ? warning.message : String(warning);
  process.emit("warning", {
    name: options.name || "Warning",
    message,
    code: options.code
  });
  return undefined;
};
