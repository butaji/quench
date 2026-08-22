//! Polyfill: `promises`


pub const JS: &str = quench_js_check::checked_js!(r##"
const __nodeTimerPromiseSchedulerError = (code, message) => {
  const error = new TypeError(message);
  error.code = code;
  return error;
};
const __nodeTimerPromiseOptions = (options) => {
  if (options === null || typeof options !== "object") {
    throw __nodeTimerPromiseSchedulerError("ERR_INVALID_ARG_TYPE", 'The "options" argument must be an object');
  }
  if (options.ref !== undefined && typeof options.ref !== "boolean") {
    throw __nodeTimerPromiseSchedulerError("ERR_INVALID_ARG_TYPE", 'The "options.ref" property must be of type boolean');
  }
  if (options.signal !== undefined && !(options.signal instanceof AbortSignal)) {
    throw __nodeTimerPromiseSchedulerError("ERR_INVALID_ARG_TYPE", 'The "options.signal" property must be an AbortSignal');
  }
  return options;
};
const __nodeTimerPromiseAbortError = (signal) => {
  const error = new Error("The operation was aborted");
  error.name = "AbortError";
  error.code = "ABORT_ERR";
  if (signal?.reason !== undefined) error.cause = signal.reason;
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
        'Value of "this" must be of type Scheduler'
      );
    }
    return globalThis.__nodeTimersPromises.setTimeout(
      _delay,
      undefined,
      options
    );
  }
  yield(options = {}) {
    if (this !== globalThis.__nodeTimersPromises.scheduler) {
      throw __nodeTimerPromiseSchedulerError(
        "ERR_INVALID_THIS",
        'Value of "this" must be of type Scheduler'
      );
    }
    return globalThis.__nodeTimersPromises.setImmediate(undefined, options);
  }
}
globalThis.__nodeTimersPromises = {
  setTimeout: (_delay = 0, value, options = {}) => {
    options = __nodeTimerPromiseOptions(options);
    return new Promise((resolve, reject) => {
      const signal = options.signal;
      if (Number.isNaN(Number(_delay))) {
        process.emitWarning("NaN is an invalid delay value", { name: "TimeoutNaNWarning" });
      }
      const abort = () => {
        globalThis.clearTimeout(timer);
        reject(__nodeTimerPromiseAbortError(signal));
      };
      if (signal?.aborted) { reject(__nodeTimerPromiseAbortError(signal)); return; }
      const timer = globalThis.setTimeout(() => {
        signal?.removeEventListener?.("abort", abort);
        resolve(value);
      }, Math.max(0, Number(_delay)));
      if (options.ref === false) timer.unref?.();
      signal?.addEventListener?.("abort", abort, { once: true });
    });
  },
  setImmediate: (value, options = {}) => {
    options = __nodeTimerPromiseOptions(options);
    return new Promise((resolve, reject) => {
      const signal = options.signal;
      let timer;
      const abort = () => {
        globalThis.clearImmediate(timer);
        reject(__nodeTimerPromiseAbortError(signal));
      };
      if (signal?.aborted) { reject(__nodeTimerPromiseAbortError(signal)); return; }
      timer = globalThis.setImmediate(() => {
        signal?.removeEventListener?.("abort", abort);
        if (!signal?.aborted) resolve(value);
      });
      if (options.ref === false) timer.unref?.();
      signal?.addEventListener?.("abort", abort, { once: true });
    });
  },
  setInterval: async function* (_delay = 0, value, options = {}) {
    options = __nodeTimerPromiseOptions(options);
    let activeAbort;
    try {
      while (true) {
        if (options.signal?.aborted) throw __nodeTimerPromiseAbortError(options.signal);
        await new Promise((resolve, reject) => {
          const abort = () => {
            globalThis.clearTimeout(timer);
            reject(__nodeTimerPromiseAbortError(options.signal));
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
        yield value;
      }
    } finally {
      options.signal?.removeEventListener?.("abort", activeAbort);
    }
  },
  scheduler: Object.create(NodeTimerPromiseScheduler.prototype)
};
process.stdout ||= { isTTY: false };
process.stderr ||= { isTTY: false };
"##);