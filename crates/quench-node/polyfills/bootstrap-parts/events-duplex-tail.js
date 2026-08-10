class NodeDuplex extends NodeReadable {
  constructor(options = {}) {
    super(options);
    this.__nodeDuplex = true;
    const writable = new NodeWritable(options);
    for (const name of "closed readableAborted writableAborted writable writableObjectMode writableHighWaterMark writableLength writableNeedDrain _writableState writableEnded writableFinished writableCorked _autoDestroy _corkedChunks _writeQueue _write _final _destroy writableDefaultEncoding".split(
      " "
    )) {
      this[name] = writable[name];
    }
    this.allowHalfOpen = options.allowHalfOpen !== false;
    if (options.readable === false) {
      this.readable = false;
      this._ended = true;
      this.readableEnded = true;
      this._readableState.ended = true;
      this._readableState.endEmitted = true;
    }
    if (options.writable === false) {
      this.writable = false;
      this._writableState.ending = true;
      this._writableState.ended = true;
      this._writableState.finished = true;
    }
  }
  destroy(error, callback) {
    this.writable = false;
    this.writableAborted = !this.writableFinished;
    return NodeReadable.prototype.destroy.call(this, error, callback);
  }
}
NodeDuplex.prototype.writableFinished = false;
const NodeDuplexCompat = function Duplex(options = {}) {
  return Reflect.construct(
    NodeDuplex,
    [options],
    new.target || NodeDuplexCompat
  );
};
NodeDuplexCompat.prototype = NodeDuplex.prototype;
for (const method of "write end cork uncork setDefaultEncoding __nodeProcessWrite".split(
  " "
)) {
  NodeDuplex.prototype[method] = NodeWritable.prototype[method];
}
const __nodeDuplexPair = (readable, writable) => {
  if (!readable && !writable) {
    throw new TypeError('The "body" argument must be a stream or iterable');
  }
  const duplex = new NodeDuplex({
    readable: Boolean(readable),
    writable: Boolean(writable),
    objectMode: Boolean(
      readable?.readableObjectMode || readable?._readableState?.objectMode
    )
  });
  duplex.readableObjectMode = Boolean(
    readable?.readableObjectMode || readable?._readableState?.objectMode
  );
  duplex._readableState.objectMode = duplex.readableObjectMode;
  duplex.writableObjectMode = Boolean(
    writable?.writableObjectMode || writable?._writableState?.objectMode
  );
  if (readable) {
    readable.on("data", (chunk) => duplex.push(chunk));
    readable.once("end", () => duplex.push(null));
    readable.once("error", (error) => {
      writable?.destroy?.(error);
      duplex.destroy(error);
    });
    duplex._read = () => readable.resume?.();
  }
  if (writable) {
    duplex._write = (chunk, encoding, callback) => {
      let settled = false;
      const finish = (error) => {
        if (settled) return;
        settled = true;
        if (error) duplex.__pairWriteError = error;
        callback(error);
      };
      const accepted = writable.write(chunk, encoding, finish);
      if (accepted !== false && !settled) finish();
    };
    duplex._final = (callback) => writable.end(undefined, undefined, callback);
    writable.once("error", (error) => {
      queueMicrotask(() => {
        if (duplex.__pairWriteError === error) {
          duplex.__pairWriteError = null;
          return;
        }
        duplex.destroy(error);
      });
    });
  }
  return duplex;
};
const __nodeDuplexFrom = (body) => {
  if (body instanceof NodeDuplex) return body;
  if (typeof body === "function") {
    if (
      body.constructor?.name === "AsyncGeneratorFunction" ||
      body.constructor?.name === "GeneratorFunction"
    ) {
      const queue = [];
      const waiters = [];
      let ended = false;
      const source = {
        [Symbol.asyncIterator]() {
          return this;
        },
        next() {
          if (queue.length) return Promise.resolve(queue.shift());
          if (ended) return Promise.resolve({ done: true });
          return new Promise((resolve) => waiters.push(resolve));
        }
      };
      const duplex = new NodeDuplex({ readable: true, writable: true });
      const pendingErrorHandler = () => {};
      pendingErrorHandler.__quenchInternal = true;
      duplex.on("error", pendingErrorHandler);
      duplex._write = (chunk, _encoding, callback) => {
        const resolve = waiters.shift();
        if (resolve) resolve({ value: chunk, done: false });
        else queue.push({ value: chunk, done: false });
        callback();
      };
      duplex._final = (callback) => {
        ended = true;
        for (const resolve of waiters.splice(0)) resolve({ done: true });
        callback();
      };
      Promise.resolve().then(async () => {
        try {
          for await (const value of body(source)) duplex.push(value);
          duplex.push(null);
        } catch (error) {
          duplex.destroy(error);
        }
      });
      return duplex;
    }
    const result = body();
    if (result === undefined) {
      throw Object.assign(new TypeError("The function must return a stream or iterable"), { code: "ERR_INVALID_RETURN_VALUE" });
    }
    return __nodeDuplexFrom(result);
  }
  if (body?.getReader || body?.getWriter) {
    return __nodeDuplexFromWeb({
      readable: body.getReader ? body : undefined,
      writable: body.getWriter ? body : undefined
    });
  }
  if (
    body &&
    typeof body === "object" &&
    (body.readable !== undefined || body.writable !== undefined) &&
    (typeof body.readable === "object" || typeof body.writable === "object")
  ) {
    if (body.readable?.getReader || body.writable?.getWriter) {
      return __nodeDuplexFromWeb(body);
    }
    return __nodeDuplexPair(body.readable, body.writable);
  }
  const readable = body?.readable === true || typeof body?.read === "function";
  const writable = body?.writable === true || typeof body?.write === "function";
  if (readable || writable) {
    return __nodeDuplexPair(readable ? body : null, writable ? body : null);
  }
  if (body && typeof body.stream === "function") {
    return __nodeDuplexFromWeb({ readable: body.stream() });
  }
  if (
    body &&
    (typeof body[Symbol.asyncIterator] === "function" ||
      typeof body[Symbol.iterator] === "function")
  ) {
    return __nodeDuplexPair(NodeReadable.from(body), null);
  }
  if (body && typeof body.then === "function") {
    const duplex = new NodeDuplex({
      readable: true,
      writable: false,
      objectMode: true
    });
    const pendingErrorHandler = () => {};
    pendingErrorHandler.__quenchInternal = true;
    duplex.on("error", pendingErrorHandler);
    Promise.resolve(body).then(
      (value) => {
        if (value !== undefined && value !== null) duplex.push(value);
        duplex.push(null);
      },
      (error) => duplex.destroy(error)
    );
    return duplex;
  }
  throw new TypeError('The "body" argument must be a stream or iterable');
};
NodeDuplex.from = __nodeDuplexFrom;
const __nodeDuplexFromWeb = (pair = {}, options = {}) => {
  const readable = pair.readable;
  const writable = pair.writable;
  const duplex = new NodeDuplex({
    ...options,
    readable: Boolean(readable),
    writable: Boolean(writable),
    read() {
      if (!this.__webReader || this.__webReading) return;
      this.__webReading = true;
      this.__webReader.read().then(
        ({ value, done }) => {
          this.__webReading = false;
          if (done) this.push(null);
          else this.push(value);
        },
        (error) => {
          this.__webReading = false;
          this.destroy(error);
        }
      );
    },
    write(chunk, _encoding, callback) {
      Promise.resolve(this.__webWriter?.write(chunk)).then(
        () => callback(),
        (error) => callback(error)
      );
    },
    final(callback) {
      Promise.resolve(this.__webWriter?.close?.()).then(
        () => callback(),
        (error) => callback(error)
      );
    },
    destroy(error, callback) {
      const reason =
        error ||
        Object.assign(new Error("The operation was aborted"), {
          name: "AbortError"
        });
      Promise.all([
        this.__webReader?.cancel(error),
        !error && (this.writableFinished || this.writableEnded)
          ? this.__webWriter?.close?.()
          : this.__webWriter?.abort?.(reason)
      ]).then(
        () => callback(),
        (destroyError) => callback(destroyError)
      );
    }
  });
  if (readable?.getReader) duplex.__webReader = readable.getReader();
  if (writable?.getWriter) duplex.__webWriter = writable.getWriter();
  return duplex;
};
NodeDuplex.fromWeb = __nodeDuplexFromWeb;
NodeDuplexCompat.from = NodeDuplex.from;
NodeDuplexCompat.fromWeb = NodeDuplex.fromWeb;
const __nodeDuplexToWeb = (duplex) => {
  const readable = new ReadableStream({
    start(controller) {
      if (duplex.readableEnded || duplex.destroyed) {
        controller.close();
        return;
      }
      duplex.on("data", (chunk) => controller.enqueue(chunk));
      duplex.once("end", () => controller.close());
      duplex.once("error", (error) => controller.error?.(error));
      duplex.resume();
    }
  });
  const writable = new WritableStream({
    write(chunk) {
      return new Promise((resolve, reject) => {
        duplex.write(chunk, (error) => (error ? reject(error) : resolve()));
      });
    },
    close() {
      return new Promise((resolve, reject) => {
        duplex.end((error) => (error ? reject(error) : resolve()));
      });
    },
    abort(error) {
      duplex.destroy(error);
    }
  });
  return { readable, writable };
};
NodeDuplex.toWeb = __nodeDuplexToWeb;
NodeDuplexCompat.toWeb = NodeDuplex.toWeb;
