//! Polyfill: `abort`

pub const JS: &str = r#"const __quenchOriginalRequireWithTransferableAbort = globalThis.require;
const __quenchTransferableSignals = new WeakSet();
const __quenchTransferableAbortSignal = (signal) => {
  if (!(signal instanceof AbortSignal)) {
    throw new TypeError("The signal argument must be an AbortSignal");
  }
  __quenchTransferableSignals.add(signal);
  return signal;
};
const __quenchTransferableAbortController = () => {
  const controller = new AbortController();
  __quenchTransferableSignals.add(controller.signal);
  return controller;
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "util") {
    return Object.assign(
      {},
      __quenchOriginalRequireWithTransferableAbort(specifier),
      {
        transferableAbortSignal: __quenchTransferableAbortSignal,
        transferableAbortController: __quenchTransferableAbortController,
      },
    );
  }
  return __quenchOriginalRequireWithTransferableAbort(specifier);
};
"#;
