(function (timers) {
  "use strict";
  function abortError() {
    return Object.assign(new Error("The operation was aborted"), {
      name: "AbortError", code: "ABORT_ERR"
    });
  }
  function promiseTimer(schedule, cancel, value, options) {
    options = options === undefined ? {} : options;
    if (!options || typeof options !== "object") {
      const error = new TypeError("The options argument must be an object");
      error.code = "ERR_INVALID_ARG_TYPE";
      return Promise.reject(error);
    }
    if (options.ref !== undefined && typeof options.ref !== "boolean") {
      const error = new TypeError("The options.ref property must be of type boolean");
      error.code = "ERR_INVALID_ARG_TYPE";
      return Promise.reject(error);
    }
    const signal = options.signal;
    if (signal !== undefined &&
        (!signal || typeof signal.addEventListener !== "function")) {
      const error = new TypeError("The signal option must be an AbortSignal");
      error.code = "ERR_INVALID_ARG_TYPE";
      return Promise.reject(error);
    }
    return new Promise(function (resolve, reject) {
      let settled = false;
      let timer;
      const cleanup = () => {
        if (signal) signal.removeEventListener("abort", onAbort);
      };
      const finish = () => {
        if (settled) return;
        settled = true;
        cleanup();
        resolve(value);
      };
      const onAbort = () => {
        if (settled) return;
        settled = true;
        cancel(timer);
        cleanup();
        reject(abortError());
      };
      timer = schedule(finish);
      if (options.ref === false && timer && typeof timer.unref === "function") timer.unref();
      if (signal) {
        if (signal.aborted) onAbort();
        else signal.addEventListener("abort", onAbort);
      }
    });
  }
  function setTimeout(delay, value, options) {
    if (options && options.signal !== undefined &&
        (!options.signal || typeof options.signal.addEventListener !== "function")) {
      const error = new TypeError("The signal option must be an AbortSignal");
      error.code = "ERR_INVALID_ARG_TYPE";
      return Promise.reject(error);
    }
    return promiseTimer(
      (finish) => timers.setTimeout(finish, delay),
      (timer) => timers.clearTimeout(timer), value, options);
  }
  function setImmediate(value, options) {
    return promiseTimer(
      (finish) => timers.setImmediate(finish),
      (timer) => timers.clearImmediate(timer), value, options);
  }
  function setInterval(delay, value, options) {
    options = options === undefined ? {} : options;
    if (!options || typeof options !== "object") {
      throw new TypeError("The options argument must be an object");
    }
    if (options.ref !== undefined && typeof options.ref !== "boolean") {
      const error = new TypeError("The options.ref property must be of type boolean");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    let timer;
    let closed = false;
    let failure = null;
    const values = [];
    const waiters = [];
    const done = { value: undefined, done: true };
    const settle = (result) => {
      if (waiters.length > 0) waiters.shift().resolve(result);
      else values.push(result.value);
    };
    const reject = (error) => {
      failure = error;
      closed = true;
      if (timer) timers.clearInterval(timer);
      removeAbortListener();
      while (waiters.length > 0) waiters.shift().reject(error);
    };
    const signal = options.signal;
    if (signal !== undefined &&
        (!signal || typeof signal.addEventListener !== "function")) {
      throw new TypeError("The signal option must be an AbortSignal");
    }
    const onAbort = () => reject(abortError());
    const removeAbortListener = () => {
      if (signal) signal.removeEventListener("abort", onAbort);
    };
    const iterator = {
      next() {
        if (values.length > 0) {
          return Promise.resolve({ value: values.shift(), done: false });
        }
        if (failure) return Promise.reject(failure);
        if (closed) return Promise.resolve(done);
        return new Promise((resolve, rejectWaiter) =>
          waiters.push({ resolve, reject: rejectWaiter }));
      },
      return() {
        if (!closed) {
          closed = true;
          if (timer) timers.clearInterval(timer);
          removeAbortListener();
          while (waiters.length > 0) waiters.shift().resolve(done);
        }
        return Promise.resolve(done);
      }
    };
    iterator[Symbol.asyncIterator] = function () { return this; };
    timer = timers.setInterval(function () {
      if (!closed) settle({ value, done: false });
    }, delay);
    if (options.ref === false && timer && typeof timer.unref === "function") timer.unref();
    if (signal) {
      if (signal.aborted) onAbort();
      else signal.addEventListener("abort", onAbort);
    }
    return iterator;
  }
  function Scheduler() {
    const error = new TypeError("Illegal constructor");
    error.code = "ERR_ILLEGAL_CONSTRUCTOR";
    throw error;
  }
  const scheduler = {};
  scheduler.constructor = Scheduler;
  scheduler.wait = function (delay, options) {
    if (this !== scheduler) {
      const error = new TypeError("Cannot read properties of an invalid Scheduler");
      error.code = "ERR_INVALID_THIS";
      throw error;
    }
    return setTimeout(delay, undefined, options);
  };
  scheduler.yield = function () {
    if (this !== scheduler) {
      const error = new TypeError("Cannot read properties of an invalid Scheduler");
      error.code = "ERR_INVALID_THIS";
      throw error;
    }
    return setImmediate(undefined);
  };
  return {
    setTimeout,
    setImmediate,
    setInterval,
    scheduler
  };
})
