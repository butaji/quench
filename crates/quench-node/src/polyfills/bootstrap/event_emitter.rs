//! Shared guest bridge for Node's EventEmitter listener semantics.

pub const JS: &str = quench_js_check::checked_js!(
    r#"(initialize, dispatch) => {
const ERROR_INSPECT = Symbol.for("quench.internal.eventEmitter.inspect");
const STREAM_OWNER = Symbol.for("quench.internal.streamOwner");
class EventEmitterBridge {
  emit(event, ...args) {
    this._events ||= Object.create(null);
    if (event === "error") {
      const monitorSymbol = globalThis.__nodeErrorMonitorSymbol ||
        Symbol.for("events.errorMonitor");
      this.rawListeners(monitorSymbol).forEach((listener) =>
        Reflect.apply(listener, this, args)
      );
    }
    const listeners = this._events[event];
    const values = listeners === undefined
      ? []
      : Array.isArray(listeners)
      ? listeners
      : [listeners];
    const internalOnlyError = event === "error" && values.length > 0 &&
      values.every((listener) => listener.__quenchInternal === true);
    const ownerWasDestroyed = this[STREAM_OWNER]?.destroyed === true;
    if (event === "error" && values.length === 0 && this.domain) {
      const error = args[0] && typeof args[0] === "object"
        ? args[0]
        : Object.assign(new Error("Unhandled error."), { domain: this.domain });
      error.domain = this.domain;
      error.domainEmitter = this;
      error.domainThrown = false;
      this.domain.emit("error", error);
      return true;
    }
    if (
      event === "error" &&
      (values.length === 0 || (internalOnlyError && ownerWasDestroyed))
    ) {
      if (internalOnlyError) {
        values.slice().filter((listener) => typeof listener === "function").forEach(
          (listener) => Reflect.apply(listener, this, args),
        );
      }
      const error = args[0];
      if (error instanceof Error) throw error;
      let detail;
      try {
        const inspect = EventEmitter[ERROR_INSPECT];
        detail = typeof inspect === "function" ? inspect(error) : String(error);
      } catch {
        detail = String(error);
      }
      const unhandled = new Error(`Unhandled error. (${detail})`);
      unhandled.code = "ERR_UNHANDLED_ERROR";
      unhandled.context = error;
      throw unhandled;
    }
    return dispatch(this, event, args, listeners);
  }

}
const prototype = EventEmitterBridge.prototype;
const EventEmitter = function EventEmitter(options) {
  'use strict';
  EventEmitter.init.call(this, options);
};
EventEmitter.prototype = prototype;
Object.defineProperty(prototype, 'constructor', {value: EventEmitter, writable: true, configurable: true});
EventEmitter.init = function init(options) {
  'use strict';
  return initialize(this, options);
};
EventEmitter.captureRejectionSymbol = Symbol.for("nodejs.rejection");
Object.defineProperty(globalThis, "__nodeEventEmitter", {
  value: EventEmitter,
  configurable: true,
});
}"#
);
