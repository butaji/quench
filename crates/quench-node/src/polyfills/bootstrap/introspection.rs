//! Polyfill: `introspection`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchEventNameVisible = (emitter, name, value) => {
  if (value === undefined || (Array.isArray(value) && value.length === 0)) {
    return false;
  }
  const hiddenError = ["NodeReadable", "NodeWritable", "NodeDuplex"].includes(
    emitter.constructor?.name,
  );
  return !(name === "error" && hiddenError && !Array.isArray(value));
};
globalThis.__nodeEventEmitter.prototype.eventNames = function () {
  const names = Reflect.ownKeys(this._events).filter((name) =>
    __quenchEventNameVisible(this, name, this._events[name])
  );
  const priority = new Map([
    ["error", 0],
    ["data", 1],
    ["prefinish", 0],
    ["drain", 1],
    ["finish", 0],
  ]);
  return names.sort(
    (left, right) => (priority.get(left) ?? 2) - (priority.get(right) ?? 2),
  );
};
globalThis.__nodeEventEmitter.prototype.off =
  globalThis.__nodeEventEmitter.prototype.removeListener;
globalThis.__nodeEventEmitter.prototype.prependListener = function (
  event,
  listener,
) {
  const current = this._events[event];
  this._events[event] = current === undefined
    ? listener
    : Array.isArray(current)
    ? [listener, ...current]
    : [listener, current];
  return this;
};
globalThis.__nodeEventEmitter.prototype.once = function (event, listener) {
  let called = false;
  const once = (...args) => {
    if (called) return;
    called = true;
    this.removeListener(event, once);
    return listener(...args);
  };
  once.listener = listener;
  return this.on(event, once);
};
globalThis.__nodeEventEmitter.prototype.prependOnceListener = function (
  event,
  listener,
) {
  let called = false;
  const once = (...args) => {
    if (called) return;
    called = true;
    this.removeListener(event, once);
    return listener(...args);
  };
  once.listener = listener;
  return this.prependListener(event, once);
};
const __quenchEventsSymbols = globalThis.require("events");
globalThis.__nodeEventEmitter.prototype.rawListeners = function (event) {
  const value = this._events[event];
  return value === undefined ? [] : Array.isArray(value) ? [...value] : [value];
};
globalThis.__nodeEventEmitter.prototype.getMaxListeners = function () {
  return __quenchEventsSymbols.getMaxListeners(this);
};
globalThis.__nodeEventEmitter.prototype.setMaxListeners = function (limit) {
  __quenchEventsSymbols.setMaxListeners(limit, this);
  return this;
};
__quenchEventsSymbols.errorMonitor ||= Symbol("events.errorMonitor");
globalThis.__nodeErrorMonitorSymbol = __quenchEventsSymbols.errorMonitor;
__quenchEventsSymbols.captureRejections ??= false;
__quenchEventsSymbols.EventEmitter.captureRejections =
  __quenchEventsSymbols.captureRejections;
"#);
