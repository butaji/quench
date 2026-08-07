const __quenchOriginalRequireWithPassThrough = globalThis.require;
class __quenchPassThrough {
  constructor() {
    this._listeners = {};
    this.readable = true;
    this.writable = true;
    this.readableEnded = false;
    this.writableEnded = false;
    this.closed = false;
  }
  on(event, callback) {
    (this._listeners[event] ||= []).push(callback);
    return this;
  }
  once(event, callback) {
    let called = false;
    const wrapped = (...args) => {
      if (called) return;
      called = true;
      this._listeners[event] = (this._listeners[event] || []).filter(
        (listener) => listener !== wrapped,
      );
      callback(...args);
    };
    return this.on(event, wrapped);
  }
  emit(event, ...args) {
    for (const callback of this._listeners[event] || []) callback(...args);
    return this;
  }
  write(chunk) {
    this.emit("data", chunk);
    return true;
  }
  read() {
    return null;
  }
  end(chunk) {
    if (chunk !== undefined) this.write(chunk);
    queueMicrotask(() => {
      this.writableEnded = true;
      this.readableEnded = true;
      this.emit("finish");
      this.emit("end");
      this.closed = true;
      this.emit("close");
    });
    return this;
  }
  pipe(destination) {
    this.on("data", (chunk) => destination.write(chunk));
    this.on("end", () => destination.end());
    return destination;
  }
}
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "stream") {
    return Object.assign(
      {},
      __quenchOriginalRequireWithPassThrough(specifier),
      { PassThrough: __quenchPassThrough },
    );
  }
  return __quenchOriginalRequireWithPassThrough(specifier);
};
