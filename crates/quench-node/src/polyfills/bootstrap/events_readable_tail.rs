//! Polyfill: `events-readable-tail`

pub const JS: &str = quench_js_check::checked_js!(r#"NodeReadable.prototype.iterator = function (options = {}) {
  if (!options || typeof options !== "object") {
    const error = new TypeError(
      `The "options" argument must be of type object. Received type ${typeof options} (${String(
        options
      )})`
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const iterator = this[Symbol.asyncIterator]();
  return {
    next: (...args) => iterator.next(...args),
    return: async (value) => {
      const result = await iterator.return?.(value);
      if (options.destroyOnReturn !== false) this.destroy();
      return result || { value, done: true };
    },
    throw: (...args) => iterator.throw?.(...args)
  };
};
NodeReadable.prototype[Symbol.asyncIterator] = function () {
  const iterator = async function* () {
    while (true) {
      if (this._chunks && this._chunks.length) {
        const value = this._chunks.shift();
        yield value;
        if (this._ended && !this._chunks.length) {
          this._emitEnd();
          return;
        }
        continue;
      }
      if (this.readableEnded || this.destroyed) {
        if (this.errored) throw this.errored;
        return;
      }
      const result = await new Promise((resolve, reject) => {
        let settled = false;
        const cleanup = () => {
          this.removeListener("data", onData);
          this.removeListener("end", onEnd);
          this.removeListener("error", onError);
          this.removeListener("close", onClose);
        };
        const finish = (value) => {
          if (settled) return;
          settled = true;
          cleanup();
          resolve(value);
        };
        const onData = (value) => finish({ type: "data", value });
        const onEnd = () => finish({ type: "end" });
        const onClose = () => {
          if (this.errored) {
            reject(this.errored);
            return;
          }
          finish({ type: "end" });
        };
        const onError = (error) => {
          if (settled) return;
          settled = true;
          cleanup();
          reject(error);
        };
        this.once("data", onData);
        this.once("end", onEnd);
        this.once("close", onClose);
        this.once("error", onError);
        __nodeReadableStart(this);
      });
      if (result.type === "end") {
        return;
      }
      yield result.value;
    }
  }.call(this);
  iterator.stream = this;
  return iterator;
};
NodeReadable.prototype.isPaused = function () {
  return this._paused;
};
Object.defineProperty(NodeReadable.prototype, "readableLength", {
  get() {
    return this._chunks.reduce(
      (length, chunk) => length + (chunk?.byteLength ?? chunk?.length ?? 1),
      0
    );
  }
});
NodeReadable.prototype.setEncoding = function (encoding) {
  encoding = String(encoding).toLowerCase();
  if (!NodeBuffer.isEncoding(encoding)) {
    throw Object.assign(new TypeError(`Unknown encoding: ${encoding}`), { code: "ERR_UNKNOWN_ENCODING" });
  }
  this.readableEncoding = encoding;
  return this;
};
NodeReadable.prototype._decode = function (chunk) {
  return this.readableEncoding && ArrayBuffer.isView(chunk)
    ? NodeBuffer.from(chunk).toString(this.readableEncoding)
    : chunk;
};
NodeReadable.prototype._emitEnd = function () {
  if (this.readable === false) return;
  if (!this.readableEnded) this.readableEnded = true;
  this._readableState.ended = true;
  this._readableState.needReadable = false;
  this._readableState.readingMore = false;
  if (!this._readableState.readingClearScheduled) {
    this._readableState.readingClearScheduled = true;
    setImmediate(() => {
      this._readableState.readingClearScheduled = false;
      this._readableState.reading = false;
    });
  }
  if (!this.listenerCount("end") || this._endEmitted) return;
  this._endEmitted = true;
  this._readableState.endEmitted = true;
  this.readable = false;
  this.emit("end");
  if (this._autoDestroy) {
    queueMicrotask(() => {
      if (!this.__nodeDuplex || this.writableFinished) this.destroy();
    });
  }
};
"#);
