//! Polyfill: `events-stream-tail`

pub const JS: &str = r#"class NodePassThrough extends NodeTransform {
  constructor(options = {}) {
    super(options);
    this.__quenchPassThroughFlow = true;
    this.__nodeDuplex = true;
    this.readableEnded = false;
    this.readableObjectMode = options.readableObjectMode ??
      options.objectMode === true;
    this.readableHighWaterMark = options.readableHighWaterMark ??
      options.highWaterMark ?? 16 * 1024;
    this._transform = undefined;
    this._write = (chunk, _encoding, callback) => {
      if (this.push(chunk)) callback();
      else this.__passThroughPendingCallback = callback;
    };
  }
  on(event, listener) {
    const result = super.on(event, listener);
    if (event === "data" && this.readableFlowing !== false) {
      queueMicrotask(() => this.resume());
    }
    return result;
  }
  pause() {
    this._paused = true;
    this.readableFlowing = false;
    return this;
  }
  resume() {
    this._paused = false;
    this.readableFlowing = true;
    while (!this._paused && this._readableChunks.length) {
      const chunk = this.read();
      if (this.listenerCount("data") > 0) this.emit("data", chunk);
    }
    if (
      !this._readableChunks.length &&
      this._writableState.finished &&
      !this.readableEnded
    ) {
      queueMicrotask(() => {
        if (!this.readableEnded) this.push(null);
      });
    }
    return this;
  }
  isPaused() {
    return this._paused === true;
  }
  push(chunk) {
    if (chunk === null && this._readableChunks.length) {
      this.__passThroughEndPending = true;
      return false;
    }
    const acceptedBySurface = super.push(chunk);
    if (chunk === null || chunk === undefined) return acceptedBySurface;
    return (
      this.listenerCount("data") > 0 ||
      this.readableLength < this.readableHighWaterMark
    );
  }
  read() {
    const chunk = super.read();
    if (
      chunk !== null &&
      !this._readableChunks.length &&
      this._writableState.finished &&
      !this.readableEnded
    ) {
      queueMicrotask(() => {
        if (!this.readableEnded) this.push(null);
      });
    }
    if (
      this.__passThroughEndPending &&
      !this._readableChunks.length &&
      !this.__passThroughEndScheduled
    ) {
      this.__passThroughEndScheduled = true;
      queueMicrotask(() => {
        this.__passThroughEndPending = false;
        this.__passThroughEndScheduled = false;
        super.push(null);
      });
    }
    if (
      this.__passThroughPendingCallback &&
      this.readableLength < this.readableHighWaterMark
    ) {
      const callback = this.__passThroughPendingCallback;
      this.__passThroughPendingCallback = null;
      callback();
    }
    return chunk;
  }
  get readableLength() {
    if (this.readableObjectMode) return this._readableChunks.length;
    return this._readableChunks.reduce(
      (length, chunk) =>
        length +
        (typeof chunk === "string"
          ? NodeBuffer.byteLength(chunk)
          : chunk?.byteLength || 0),
      0,
    );
  }
}
const NodeStream = function NodeStream() {
  this._events = Object.create(null);
};
NodeStream.prototype = Object.create(NodeEventEmitter.prototype);
NodeStream.prototype.constructor = NodeStream;
NodeStream.prototype.write = () => true;
NodeStream.prototype.end = function () {
  this.emit("finish");
  return this;
};
NodeStream.prototype.pipe = function (destination) {
  destination.emit?.("pipe", this);
  this.on("data", (chunk) => destination.write(chunk));
  this.on("end", () => destination.end());
  return destination;
};
"#;
