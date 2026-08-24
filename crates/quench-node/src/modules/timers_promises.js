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
  return { setTimeout: setTimeoutPromise, setImmediate: setImmediatePromise };
})
