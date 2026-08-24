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
  function setIntervalPromise(delay, value) {
    let timer;
    let closed = false;
    const values = [];
    const waiters = [];
    const done = { value: undefined, done: true };
    const settle = (result) => {
      if (waiters.length > 0) waiters.shift()(result);
      else values.push(result.value);
    };
    const iterator = {
      next() {
        if (values.length > 0) {
          return Promise.resolve({ value: values.shift(), done: false });
        }
        if (closed) return Promise.resolve(done);
        return new Promise((resolve) => waiters.push(resolve));
      },
      return() {
        if (!closed) {
          closed = true;
          if (timer) timers.clearInterval(timer);
          while (waiters.length > 0) waiters.shift()(done);
        }
        return Promise.resolve(done);
      }
    };
    iterator[Symbol.asyncIterator] = function () { return this; };
    timer = timers.setInterval(function () {
      if (!closed) settle({ value, done: false });
    }, delay);
    return iterator;
  }
  return {
    setTimeout: setTimeoutPromise,
    setImmediate: setImmediatePromise,
    setInterval: setIntervalPromise
  };
})
