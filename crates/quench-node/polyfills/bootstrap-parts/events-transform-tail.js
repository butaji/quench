class NodeTransform extends NodeWritable {
  constructor(options = {}) {
    super(options);
    this.readable = options.readable !== false;
    this.writable = options.writable !== false;
    this.readableObjectMode = options.readableObjectMode ??
      options.objectMode === true;
    this.readableHighWaterMark = options.readableHighWaterMark ??
      options.highWaterMark ?? 16 * 1024;
    this._readableChunks = [];
    this._transformEndEmitted = false;
    this._transformFlowScheduled = false;
    if (typeof options.transform === "function") {
      this._transform = options.transform;
    }
    this.once("finish", () => {
      if (this.readable !== false && !this.readableEnded) this.push(null);
    });
  }
  once(event, listener) {
    let called = false;
    const wrapped = (...args) => {
      if (called) return;
      called = true;
      this.removeListener?.(event, wrapped);
      listener(...args);
    };
    return this.on(event, wrapped);
  }
  on(event, listener) {
    const result = super.on(event, listener);
    if (
      (event === "data" || event === "end") &&
      !this.__quenchPassThroughFlow &&
      !this._transformFlowScheduled
    ) {
      this._transformFlowScheduled = true;
      queueMicrotask(() => {
        this._transformFlowScheduled = false;
        while (this._readableChunks.length && this.listenerCount("data")) {
          this.emit("data", this._readableChunks.shift());
        }
        if (
          this.readableEnded &&
          !this._readableChunks.length &&
          !this._transformEndEmitted &&
          this.listenerCount("end")
        ) {
          this._transformEndEmitted = true;
          this.emit("end");
        }
      });
    }
    return result;
  }
  push(chunk) {
    if (chunk === null) {
      if (!this.readableEnded) {
        this.readableEnded = true;
        if (
          !this._readableChunks.length &&
          !this._transformEndEmitted &&
          this.listenerCount("end")
        ) {
          this._transformEndEmitted = true;
          this.emit("end");
        }
        if (this._autoDestroy && this._writableState.finished) {
          queueMicrotask(() => {
            if (!this.destroyed) this.destroy();
          });
        }
      }
      return false;
    }
    if (this.readable === false) {
      const error = new Error("stream.push() after EOF");
      error.code = "ERR_STREAM_PUSH_AFTER_EOF";
      queueMicrotask(() => this.emit("error", error));
      return false;
    }
    if (chunk !== undefined) {
      const value = !this.readableObjectMode && typeof chunk === "string"
        ? NodeBuffer.from(chunk)
        : chunk;
      if (this.listenerCount("data")) this.emit("data", value);
      else if (!this._transformResumed) {
        this._readableChunks.push(value);
        if (this.listenerCount("readable")) {
          queueMicrotask(() => this.emit("readable"));
        }
      }
    }
    return chunk !== null;
  }
  resume() {
    this.readableFlowing = true;
    this._transformResumed = true;
    while (this._readableChunks.length) {
      const chunk = this._readableChunks.shift();
      if (this.listenerCount("data")) this.emit("data", chunk);
    }
    if (
      this.readableEnded &&
      !this._transformEndEmitted &&
      this.listenerCount("end")
    ) {
      this._transformEndEmitted = true;
      queueMicrotask(() => this.emit("end"));
    }
    return this;
  }
  read() {
    const value = this._readableChunks.shift() ?? null;
    if (
      value === null &&
      this.readableEnded &&
      !this._transformEndEmitted &&
      this.listenerCount("end")
    ) {
      this._transformEndEmitted = true;
      queueMicrotask(() => this.emit("end"));
    }
    return value;
  }
  write(chunk, encoding, callback) {
    if (typeof encoding === "function") {
      callback = encoding;
      encoding = "utf8";
    }
    if (this._transform) {
      const size = typeof chunk === "string"
        ? NodeBuffer.byteLength(chunk)
        : chunk?.byteLength || 1;
      this.writableLength += size;
      this._writableState.writing = true;
      const complete = (error) => {
        if (!this._writableState.writing) {
          const duplicate = new Error("Callback called multiple times");
          duplicate.code = "ERR_MULTIPLE_CALLBACK";
          queueMicrotask(() => this.emit("error", duplicate));
          return;
        }
        this._writableState.writing = false;
        this.writableLength = Math.max(0, this.writableLength - size);
        if (
          this.writableNeedDrain &&
          (this.writableLength === 0 ||
            this.writableLength < this.writableHighWaterMark)
        ) {
          this.writableNeedDrain = false;
          this._writableState.needDrain = false;
          this.emit("drain");
        }
        if (error) this.emit("error", error);
        if (callback) callback(error);
        this.__nodeMaybeFinish?.();
      };
      this.writableNeedDrain =
        this.writableLength >= this.writableHighWaterMark;
      this._writableState.needDrain = this.writableNeedDrain;
      this._transform.call(this, chunk, encoding, (error, output) => {
        if (error) {
          this.destroy(error);
          complete(error);
          return;
        }
        if (output !== undefined) this.push(output);
        complete();
      });
      return !this.writableNeedDrain;
    } else return super.write(chunk, encoding, callback);
  }
  pipe(destination, options = {}) {
    this.on("data", (chunk) => {
      if (destination.write(chunk) === false) {
        this.pause?.();
        destination.once("drain", () => this.resume?.());
      }
    });
    if (options.end !== false) this.on("end", () => destination.end());
    return destination;
  }
  [Symbol.asyncIterator]() {
    if (this.readable === false) return (async function* () {})();
    return (async function* (stream) {
      while (true) {
        if (stream._readableChunks.length) {
          yield stream._readableChunks.shift();
          continue;
        }
        if (stream.readableEnded || stream.destroyed) return;
        const item = await new Promise((resolve, reject) => {
          const cleanup = () => {
            stream.removeListener("data", onData);
            stream.removeListener("end", onEnd);
            stream.removeListener("error", onError);
          };
          const onData = (value) => {
            cleanup();
            resolve({ value, done: false });
          };
          const onEnd = () => {
            cleanup();
            resolve({ value: undefined, done: true });
          };
          const onError = (error) => {
            cleanup();
            reject(error);
          };
          stream.once("data", onData);
          stream.once("end", onEnd);
          stream.once("error", onError);
        });
        if (item.done) return;
        yield item.value;
      }
    })(this);
  }
}
