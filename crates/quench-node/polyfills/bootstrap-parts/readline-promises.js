const __quenchOriginalRequireWithReadlinePromises = globalThis.require;
class __quenchReadlinePromisesInterface {
  constructor(options = {}) {
    this.input = options.input;
    this.output = options.output;
    this._closed = false;
  }
  question(prompt) {
    if (this._closed) return Promise.reject(new Error("Interface is closed"));
    this.output?.write?.(prompt);
    return new Promise((resolve) => this.input?.once?.("line", resolve));
  }
  close() {
    this._closed = true;
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
