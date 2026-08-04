globalThis.__nodeEventEmitter.prototype.eventNames = function () {
  return Reflect.ownKeys(this._events).filter(
    (name) => (this._events[name] || []).length > 0
  );
};
globalThis.__nodeEventEmitter.prototype.off =
  globalThis.__nodeEventEmitter.prototype.removeListener;
globalThis.__nodeEventEmitter.prototype.prependListener = function (
  event,
  listener
) {
  (this._events[event] ||= []).unshift(listener);
  return this;
};
globalThis.__nodeEventEmitter.prototype.once = function (event, listener) {
  const once = (...args) => {
    this.removeListener(event, once);
    return listener(...args);
  };
  once.listener = listener;
  return this.on(event, once);
};
globalThis.__nodeEventEmitter.prototype.prependOnceListener = function (
  event,
  listener
) {
  const once = (...args) => {
    this.removeListener(event, once);
    return listener(...args);
  };
  once.listener = listener;
  return this.prependListener(event, once);
};
const __quenchEventsSymbols = globalThis.require("events");
globalThis.__nodeEventEmitter.prototype.rawListeners = function (event) {
  return [...(this._events[event] || [])];
};
globalThis.__nodeEventEmitter.prototype.getMaxListeners = function () {
  return __quenchEventsSymbols.getMaxListeners(this);
};
globalThis.__nodeEventEmitter.prototype.setMaxListeners = function (limit) {
  __quenchEventsSymbols.setMaxListeners(limit, this);
  return this;
};
__quenchEventsSymbols.errorMonitor ||= Symbol("events.errorMonitor");
__quenchEventsSymbols.captureRejections ??= false;
__quenchEventsSymbols.EventEmitter.captureRejections =
  __quenchEventsSymbols.captureRejections;
