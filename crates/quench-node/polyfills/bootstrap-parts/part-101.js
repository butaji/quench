globalThis.__quench_bootstrap_fragments.push(
  'globalThis.__nodeEventEmitter.prototype.eventNames = function () { return Reflect.ownKeys(this._events).filter((name) => (this._events[name] || []).length > 0); };\nconst __quenchEventsSymbols = globalThis.require("events");\n__quenchEventsSymbols.errorMonitor ||= Symbol("events.errorMonitor");\n__quenchEventsSymbols.captureRejections ??= false;\n__quenchEventsSymbols.EventEmitter.captureRejections = __quenchEventsSymbols.captureRejections;\n'
);
