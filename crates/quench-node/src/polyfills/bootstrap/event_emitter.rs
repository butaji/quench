//! Shared guest bridge for Node's EventEmitter listener semantics.

pub const JS: &str = quench_js_check::checked_js!(
    r#"(initialize, dispatch, errorInspector) => {
const STREAM_OWNER = Symbol.for("quench.internal.streamOwner");
class EventEmitterBridge {
  emit(event, args, listeners) {
    const values = listeners === undefined
      ? []
      : Array.isArray(listeners)
      ? listeners
      : [listeners];
    const internalOnlyError = event === "error" && values.length > 0 &&
      values.every((listener) => listener.__quenchInternal === true);
    const ownerWasDestroyed = this[STREAM_OWNER]?.destroyed === true;
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
        detail = errorInspector()(error);
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
// Node assigns these anonymous functions, so they have no name.
EventEmitter.init = function(options) {
  'use strict';
  return initialize(this, options);
};
EventEmitter.captureRejectionSymbol = Symbol.for("nodejs.rejection");
EventEmitter.usingDomains = false;
Object.defineProperty(globalThis, "__nodeEventEmitter", {
  value: EventEmitter,
  configurable: true,
});
}"#
);
