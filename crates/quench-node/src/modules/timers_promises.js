(function (timers) {
  "use strict";
  function setTimeoutPromise(delay, value) {
    return new Promise(function (resolve) {
      timers.setTimeout(resolve, delay, value);
    });
  }
  function setImmediatePromise(value) {
    return new Promise(function (resolve) {
      timers.setImmediate(resolve, value);
    });
  }
  function abortError() {
    return Object.assign(new Error("The operation was aborted"), {
      name: "AbortError", code: "ABORT_ERR"
    });
  }
  function setIntervalPromise(delay, value, options) {
    options = options === undefined ? {} : options;
    if (!options || typeof options !== "object") {
      throw new TypeError("The options argument must be an object");
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
      while (waiters.length > 0) waiters.shift().reject(error);
    };
    const signal = options.signal;
    if (signal !== undefined &&
        (!signal || typeof signal.addEventListener !== "function")) {
      throw new TypeError("The signal option must be an AbortSignal");
    }
    const onAbort = () => reject(abortError());
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
          if (signal) signal.removeEventListener("abort", onAbort);
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
  return {
    setTimeout: setTimeoutPromise,
    setImmediate: setImmediatePromise,
    setInterval: setIntervalPromise
  };
})
