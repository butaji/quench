//! Polyfill: `events-tail`

pub const JS: &str = quench_js_check::checked_js!(r#"const __nodeDuplexPairFactory = (options = {}) => {
  const left = new NodeDuplex(options);
  const right = new NodeDuplex(options);
  const connect = (source, destination) => {
    source.__nodePairPending = [];
    source._write = (chunk, encoding, callback) => {
      if (source.writableCorked > 0) {
        source.__nodePairPending.push([chunk, encoding, callback]);
      } else {
        destination.push(chunk, encoding);
        callback?.();
      }
    };
    source.uncork = () => {
      if (source.writableCorked > 0) source.writableCorked--;
      if (source.writableCorked !== 0) return;
      const pending = source.__nodePairPending.splice(0);
      for (const [chunk, encoding, callback] of pending) {
        destination.push(chunk, encoding);
        callback?.();
      }
    };
    source._final = (callback) => {
      const flush = () => {
        const pending = source.__nodePairPending.splice(0);
        for (const [chunk, encoding, writeCallback] of pending) {
          destination.push(chunk, encoding);
          writeCallback?.();
        }
        destination.push(null);
        callback?.();
      };
      if (source.writableCorked > 0) source.writableCorked = 0;
      queueMicrotask(flush);
    };
  };
  connect(left, right);
  connect(right, left);
  return [left, right];
};
const NodeReadableCompat = function Readable(options = {}) {
  const instance = Reflect.construct(NodeReadable, [
    { ...options, __quenchCompatConstruct: true },
  ]);
  if (this instanceof NodeReadableCompat && this !== instance) {
    Object.assign(this, instance);
    if (this._autoDestroy) {
      const autoDestroyErrorListener = () => {
        if (!this.destroyed) this.destroy();
      };
      autoDestroyErrorListener.__quenchInternal = true;
      this.on("error", autoDestroyErrorListener);
    }
    return this;
  }
  return instance;
};
NodeReadableCompat.prototype = NodeReadable.prototype;
Object.defineProperty(NodeReadableCompat, "from", {
  value: NodeReadable.from,
  configurable: true,
  writable: true,
});
const __nodePipeline = (...args) => {
  const callback = args.pop();
  if (args.length === 0 || typeof callback !== "function") {
    throw Object.assign(new TypeError("The pipeline callback must be a function"), { code: "ERR_INVALID_ARG_TYPE" });
  }
  const streams = [];
  for (let index = 0; index < args.length; index++) {
    const stream = args[index];
    if (typeof stream === "function") {
      if (index > 0 && stream.constructor?.name === "GeneratorFunction") {
        throw Object.assign(new TypeError("The function must return an async iterable or stream"), { code: "ERR_INVALID_RETURN_VALUE" });
      }
      if (stream.constructor?.name === "AsyncGeneratorFunction") {
        streams.push(NodeDuplex.from(stream));
        continue;
      }
      const result = stream(streams[index - 1]);
      if (result === undefined) {
        throw Object.assign(new TypeError("The function must return a stream or iterable"), { code: "ERR_INVALID_RETURN_VALUE" });
      }
      streams.push(NodeDuplex.from(result));
      continue;
    }
    if (
      index === 0 &&
      !stream?.pipe &&
      (typeof stream === "string" ||
        stream?.[Symbol.iterator] ||
        stream?.[Symbol.asyncIterator])
    ) {
      streams.push(NodeReadable.from(stream));
      continue;
    }
    streams.push(stream);
  }
  if (streams.length < 2) {
    throw Object.assign(new TypeError("The pipeline requires at least two streams"), { code: "ERR_MISSING_ARGS" });
  }
  let settled = false;
  const complete = (error) => {
    if (settled) return;
    settled = true;
    callback(error);
  };
  for (const stream of streams) stream.once?.("error", complete);
  for (let index = 0; index + 1 < streams.length; index++) {
    streams[index].pipe?.(streams[index + 1]);
  }
  const destination = streams[streams.length - 1];
  if (destination.writable !== false) {
    destination.once?.("finish", () => complete());
  } else {
    destination.once?.("end", () => complete());
  }
  return destination;
};
const NodePassThroughCompat = function PassThrough(options = {}) {
  return Reflect.construct(
    NodePassThrough,
    [options],
    new.target || NodePassThroughCompat,
  );
};
NodePassThroughCompat.prototype = NodePassThrough.prototype;
const __nodeStreamExports = {
  Stream: NodeStream,
  Readable: NodeReadableCompat,
  Writable: NodeWritableCompat,
  destroy: (stream, error, callback) => {
    if (typeof error === "function") {
      callback = error;
      error = undefined;
    }
    if (error === undefined && stream && !stream.destroyed) {
      error = new Error("The operation was aborted");
      error.name = "AbortError";
    }
    stream?.destroy?.(error, callback);
    return stream;
  },
  Duplex: NodeDuplexCompat,
  duplexPair: __nodeDuplexPairFactory,
  pipeline: __nodePipeline,
  Transform: NodeTransform,
  PassThrough: NodePassThroughCompat,
  finished: (stream, options, callback) => {
    if (typeof options === "function") {
      callback = options;
      options = {};
    }
    if (options == null) options = {};
    if (typeof options !== "object") {
      throw Object.assign(
        new TypeError("The options argument must be an object"),
        {
          code: "ERR_INVALID_ARG_TYPE",
        },
      );
    }
    if (typeof callback !== "function") {
      throw Object.assign(
        new TypeError("The callback argument must be a function"),
        {
          code: "ERR_INVALID_ARG_TYPE",
        },
      );
    }
    if (typeof stream?.getReader === "function" && stream._closedPromise) {
      stream._closedPromise.then(() => callback(), callback);
      return () => {};
    }
    const readable = options.readable !== false &&
      stream?.readable !== false &&
      (stream?.readable !== undefined || stream?.readableEnded !== undefined);
    const writable = options.writable !== false &&
      stream?.writable !== false &&
      (stream?.writable !== undefined || stream?.writableEnded !== undefined);
    let ended = !readable;
    let finished = !writable;
    let settled = false;
    const done = (error) => {
      if (settled) return;
      settled = true;
      stream.removeListener?.("end", onEnd);
      stream.removeListener?.("finish", onFinish);
      stream.removeListener?.("error", onError);
      callback(error);
    };
    const settle = () => {
      if (ended && finished) done();
    };
    const onEnd = () => {
      ended = true;
      settle();
    };
    const onFinish = () => {
      finished = true;
      settle();
    };
    const onError = (error) => done(error);
    stream.once?.("end", onEnd);
    stream.once?.("finish", onFinish);
    stream.once?.("error", onError);
    settle();
    return () => {
      stream.removeListener?.("end", onEnd);
      stream.removeListener?.("finish", onFinish);
      stream.removeListener?.("error", onError);
    };
  },
  isDisturbed: (stream) => Boolean(stream?._readableState?.dataEmitted),
  isErrored: (stream) =>
    Boolean(stream?.errored || stream?._readableState?.errored),
};
globalThis.__nodeStreamInitialized = false;
globalThis.__nodeStream = new Proxy(__nodeStreamExports, {
  get: (target, key) => {
    globalThis.__nodeStreamInitialized = true;
    return target[key];
  },
});
"#);
