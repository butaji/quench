//! Shared-VM projection of the existing Node EventEmitter bridge.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const EVENTS_API: &str = quench_js_check::checked_js!(
    r#"(() => {
  const invalid = (message) => {
    const error = new TypeError(message);
    error.code = "ERR_INVALID_ARG_TYPE";
    return error;
  };

  const once = (emitter, event, options) => {
    if (options !== undefined && (options === null || typeof options !== "object")) {
      return Promise.reject(invalid("The options argument must be an object"));
    }
    options ||= {};
    const signal = options.signal;
    if (signal !== undefined &&
        (signal === null || typeof signal !== "object" ||
         typeof signal.addEventListener !== "function")) {
      return Promise.reject(invalid("The signal option must be an AbortSignal"));
    }
    const isEmitter = typeof emitter?.once === "function";
    if (!isEmitter && typeof emitter?.addEventListener !== "function") {
      return Promise.reject(invalid("The emitter must be an EventEmitter or EventTarget"));
    }
    return new Promise((resolve, reject) => {
      const remove = () => {
        if (isEmitter) {
          emitter.removeListener?.(event, onEvent);
          if (event !== "error") emitter.removeListener?.("error", onError);
        } else {
          emitter.removeEventListener?.(event, onEvent);
        }
        signal?.removeEventListener?.("abort", onAbort);
      };
      const onEvent = (...args) => { remove(); resolve(args); };
      const onError = (error) => { remove(); reject(error); };
      const onAbort = () => {
        remove();
        const error = new Error("The operation was aborted");
        error.name = "AbortError";
        error.code = "ABORT_ERR";
        reject(error);
      };
      if (isEmitter) {
        emitter.once(event, onEvent);
        if (event !== "error") emitter.once("error", onError);
      } else {
        emitter.addEventListener(event, onEvent, { once: true });
      }
      if (signal?.aborted) onAbort();
      else signal?.addEventListener?.("abort", onAbort, { once: true });
      queueMicrotask(() => { if (signal?.aborted) onAbort(); });
    });
  };

  const listenerCount = (emitter, event) => {
    if (typeof emitter?.listenerCount === "function") return emitter.listenerCount(event);
    return globalThis.__quenchAbortListenerCount?.(emitter, event) ?? 0;
  };

  return { once, listenerCount };
})()"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let global = context.global_root()?;
    let class_key = context.string_rooted("__nodeEventEmitter");
    let module = context.get_property_rooted(global, class_key)?;
    set(context, module, "EventEmitter", module)?;
    set(context, module, "default", module)?;

    let api = context.evaluate_script_rooted(EVENTS_API, "node:events/shared-api.js")?;
    for name in ["once", "listenerCount"] {
        let key = context.string_rooted(name);
        let value = context.get_property_rooted(api, key)?;
        set(context, module, name, value)?;
    }
    let symbol = context.evaluate_script_rooted(
        "Symbol.for('events.errorMonitor')",
        "node:events/errorMonitor",
    )?;
    set(context, module, "errorMonitor", symbol)?;
    let capture_symbol = context.evaluate_script_rooted(
        "Symbol.for('nodejs.rejection')",
        "node:events/captureRejectionSymbol",
    )?;
    set(context, module, "captureRejectionSymbol", capture_symbol)?;
    let default_max_listeners = context.number(10.0);
    set(
        context,
        module,
        "defaultMaxListeners",
        default_max_listeners,
    )?;
    Ok(module)
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!(
            "cannot install shared events property {name}"
        )))
    }
}
