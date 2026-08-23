//! Polyfill: `readline-promises`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithReadlinePromises = globalThis.require;
class __quenchReadlinePromisesInterface {
  constructor(options = {}) {
    this.input = options.input;
    this.output = options.output;
    this._closed = false;
    this._pendingQuestions = new Set();
  }
  question(prompt, options = {}) {
    if (this._closed) return Promise.reject(new Error("Interface is closed"));
    const signal = options?.signal;
    const abortError = () => {
      const error = new Error("The operation was aborted");
      error.name = "AbortError";
      error.code = "ABORT_ERR";
      return error;
    };
    if (signal?.aborted) return Promise.reject(abortError());
    this.output?.write?.(prompt);
    return new Promise((resolve, reject) => {
      let settled = false;
      let pendingReject;
      const finish = (fn, value) => {
        if (settled) return;
        settled = true;
        if (pendingReject) this._pendingQuestions.delete(pendingReject);
        signal?.removeEventListener?.("abort", onAbort);
        fn(value);
      };
      const onAbort = () => finish(reject, abortError());
      pendingReject = () => finish(reject, closedError());
      this._pendingQuestions.add(pendingReject);
      signal?.addEventListener?.("abort", onAbort, { once: true });
      if (signal?.aborted) return onAbort();
      this.input?.once?.("line", (answer) => finish(resolve, answer));
    });
  }
  close() {
    if (this._closed) return;
    this._closed = true;
    this._pendingQuestions.forEach((rejectQuestion) => rejectQuestion());
    this._pendingQuestions.clear();
    this.input?.pause?.();
  }
  prompt() {}
  write() {}
  pause() {
    return this;
  }
  resume() {
    return this;
  }
  [Symbol.asyncIterator]() {
    return { next: async () => ({ value: undefined, done: true }) };
  }
}
const closedError = () => new Error("Interface is closed");
const __quenchReadlinePromises = {
  Interface: __quenchReadlinePromisesInterface,
  createInterface: (options) => new __quenchReadlinePromisesInterface(options),
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "readline/promises") {
    return __quenchReadlinePromises;
  }
  return __quenchOriginalRequireWithReadlinePromises(specifier);
};
"#);
