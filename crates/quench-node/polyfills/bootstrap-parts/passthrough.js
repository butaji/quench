const __quenchOriginalRequireWithPassThrough = globalThis.require;
class __quenchPassThrough {
  constructor() {
    this._listeners = {};
    this.readable = true;
    this.writable = true;
  }
  on(event, callback) {
    (this._listeners[event] ||= []).push(callback);
    return this;
  }
  emit(event, ...args) {
    for (const callback of this._listeners[event] || []) callback(...args);
    return this;
  }
  write(chunk) {
    this.emit("data", chunk);
    return true;
  }
  end(chunk) {
    if (chunk !== undefined) this.write(chunk);
    queueMicrotask(() => this.emit("end"));
    return this;
  }
  pipe(destination) {
    this.on("data", (chunk) => destination.write(chunk));
    this.on("end", () => destination.end());
    return destination;
  }
}
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "stream")
    return Object.assign(
      {},
      __quenchOriginalRequireWithPassThrough(specifier),
      { PassThrough: __quenchPassThrough }
    );
  return __quenchOriginalRequireWithPassThrough(specifier);
};
