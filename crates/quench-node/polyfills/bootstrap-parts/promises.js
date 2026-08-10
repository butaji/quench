const __nodeTimerPromiseSchedulerError = (code, message) => {
  const error = new TypeError(message);
  error.code = code;
  return error;
};
class NodeTimerPromiseScheduler {
  constructor() {
    const error = new Error("Illegal constructor");
    error.code = "ERR_ILLEGAL_CONSTRUCTOR";
    throw error;
  }
  wait(_delay = 0, options = {}) {
    if (this !== globalThis.__nodeTimersPromises.scheduler) {
      throw __nodeTimerPromiseSchedulerError(
        "ERR_INVALID_THIS",
        'Value of "this" must be of type Scheduler',
      );
    }
    return globalThis.__nodeTimersPromises.setTimeout(
      _delay,
      undefined,
      options,
    );
  }
  yield(options = {}) {
    if (this !== globalThis.__nodeTimersPromises.scheduler) {
      throw __nodeTimerPromiseSchedulerError(
        "ERR_INVALID_THIS",
        'Value of "this" must be of type Scheduler',
      );
    }
    return globalThis.__nodeTimersPromises.setImmediate(undefined, options);
  }
}
globalThis.__nodeTimersPromises = {
  setTimeout: (_delay = 0, value, options = {}) =>
    new Promise((resolve, reject) => {
      const signal = options?.signal;
      if (Number.isNaN(Number(_delay))) {
        process.emitWarning("NaN is an invalid delay value", {
          name: "TimeoutNaNWarning",
        });
      }
      const abort = () => {
        const error = new Error("The operation was aborted");
        error.name = "AbortError";
        error.code = "ABORT_ERR";
        reject(error);
      };
      if (signal?.aborted) {
        abort();
        return;
      }
      const timer = globalThis.setTimeout(() => {
        signal?.removeEventListener?.("abort", abort);
        resolve(value);
      }, Math.max(0, Number(_delay)));
      signal?.addEventListener?.("abort", () => {
        globalThis.clearTimeout(timer);
        abort();
      }, { once: true });
    }),
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
    if (options === null || typeof options !== "object") {
      throw __nodeTimerPromiseSchedulerError(
        "ERR_INVALID_ARG_TYPE",
        'The "options" argument must be an object',
      );
    }
    if (options.ref !== undefined && typeof options.ref !== "boolean") {
      throw __nodeTimerPromiseSchedulerError(
        "ERR_INVALID_ARG_TYPE",
        'The "options.ref" property must be of type boolean',
      );
    }
    if (
      options.signal !== undefined &&
      (options.signal === null ||
        typeof options.signal.addEventListener !== "function")
    ) {
      throw __nodeTimerPromiseSchedulerError(
        "ERR_INVALID_ARG_TYPE",
        'The "options.signal" property must be an AbortSignal',
      );
    }
    let activeAbort;
    try {
      while (true) {
        if (options.signal?.aborted) {
          const error = new Error("The operation was aborted");
          error.name = "AbortError";
          error.code = "ABORT_ERR";
          if (options.signal.reason !== undefined) {
            error.cause = options.signal.reason;
          }
          throw error;
        }
        if (Number(_delay) > 0) {
          await new Promise((resolve, reject) => {
            const abort = () => {
              globalThis.clearTimeout(timer);
              const error = new Error("The operation was aborted");
              error.name = "AbortError";
              error.code = "ABORT_ERR";
              if (options.signal.reason !== undefined) {
                error.cause = options.signal.reason;
              }
              reject(error);
            };
            activeAbort = abort;
            const timer = globalThis.setTimeout(() => {
              options.signal?.removeEventListener?.("abort", abort);
              activeAbort = undefined;
              resolve();
            }, Math.max(0, Number(_delay)));
            if (options.ref === false) timer.unref?.();
            options.signal?.addEventListener?.("abort", abort, { once: true });
          });
        } else await new Promise((resolve) => queueMicrotask(resolve));
        yield value;
      }
    } finally {
      options.signal?.removeEventListener?.("abort", activeAbort);
    }
  },
  scheduler: Object.create(NodeTimerPromiseScheduler.prototype),
};

const processListeners = {};
process.stdout ||= { isTTY: false };
process.stderr ||= { isTTY: false };
process.on = (event, listener) => {
  (processListeners[event] ||= []).push(listener);
  if (event === "uncaughtException" && globalThis.__quench_pending_uncaught) {
    const error = globalThis.__quench_pending_uncaught;
    globalThis.__quench_pending_uncaught = undefined;
    queueMicrotask(() => listener(error));
  }
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
    (item) => item !== listener,
  );
  return process;
};
process.removeAllListeners = (event) => {
  if (event) delete processListeners[event];
  else {
    Object.keys(processListeners).forEach(
      (key) => delete processListeners[key],
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
    code: options.code,
  });
  return undefined;
};
