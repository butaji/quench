const __quenchRequireStreamIter = () => {
  const toStreamable = Symbol.for("nodejs.stream.iter.toStreamable");
  const toAsyncStreamable = Symbol.for("nodejs.stream.iter.toAsyncStreamable");
  const normalizeOptions = (options) => {
    if (options === undefined) return {};
    if (!options || typeof options !== "object") {
      const error = new TypeError("options must be an object");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (
      options.limit !== undefined &&
      (!Number.isInteger(options.limit) || options.limit < 0)
    ) {
      const error = new RangeError("limit must be a non-negative integer");
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (
      options.signal !== undefined &&
      (!options.signal || typeof options.signal !== "object")
    ) {
      const error = new TypeError("signal must be an AbortSignal");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return options;
  };
  const abortError = (signal) =>
    signal?.reason ||
    Object.assign(new Error("The operation was aborted"), {
      name: "AbortError"
    });
  const normalizeChunk = (chunk) => {
    if (chunk === null) return [];
    if (Array.isArray(chunk)) return chunk.flatMap(normalizeChunk);
    if (typeof chunk === "string") return [NodeBuffer.from(chunk)];
    if (chunk instanceof Uint8Array || ArrayBuffer.isView(chunk)) {
      return [
        NodeBuffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength)
      ];
    }
    if (chunk instanceof ArrayBuffer) return [NodeBuffer.from(chunk)];
    return [NodeBuffer.from(chunk)];
  };
  const sourceAsync = async function* (source) {
    if (typeof source === "string") {
      yield [NodeBuffer.from(source)];
      return;
    }
    if (source instanceof ArrayBuffer || ArrayBuffer.isView(source)) {
      yield [
        NodeBuffer.from(
          source.buffer || source,
          source.byteOffset || 0,
          source.byteLength
        )
      ];
      return;
    }
    if (typeof source?.toAsyncStream === "function") {
      yield* sourceAsync(await source.toAsyncStream());
      return;
    }
    if (typeof source?.[toAsyncStreamable] === "function") {
      yield* sourceAsync(await source[toAsyncStreamable]());
      return;
    }
    if (typeof source?.[toStreamable] === "function") {
      yield* sourceAsync(source[toStreamable]());
      return;
    }
    if (source?.[Symbol.asyncIterator]) {
      yield* source;
      return;
    }
    if (source?.[Symbol.iterator]) {
      yield* source;
      return;
    }
    throw new TypeError("source must be iterable");
  };
  const normalizeSyncValue = function* (value) {
    if (typeof value === "string") {
      yield NodeBuffer.from(value);
      return;
    }
    if (value instanceof ArrayBuffer || ArrayBuffer.isView(value)) {
      yield value;
      return;
    }
    if (typeof value?.[toStreamable] === "function") {
      yield* normalizeSyncValue(value[toStreamable]());
      return;
    }
    if (Array.isArray(value) || value?.[Symbol.iterator]) {
      for (const item of value) yield* normalizeSyncValue(item);
      return;
    }
    const error = new TypeError("source values must be streamable");
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  };
  const sourceSync = function* (source) {
    if (
      typeof source === "string" ||
      source instanceof ArrayBuffer ||
      ArrayBuffer.isView(source)
    ) {
      yield [...normalizeSyncValue(source)];
      return;
    }
    if (typeof source?.[toStreamable] === "function") {
      yield* sourceSync(source[toStreamable]());
      return;
    }
    if (!source?.[Symbol.iterator]) {
      const error = new TypeError("source must be a synchronous iterable");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    let batch = [];
    for (const value of source) {
      if (value instanceof Uint8Array) {
        batch.push(value);
        continue;
      }
      if (batch.length) {
        yield batch;
        batch = [];
      }
      const normalized = [...normalizeSyncValue(value)];
      if (normalized.length) yield normalized;
    }
    if (batch.length) yield batch;
  };
  const readAsync = async (source, options = {}) => {
    const signal = options.signal;
    if (signal?.aborted) throw abortError(signal);
    const iterator = sourceAsync(source)[Symbol.asyncIterator]();
    const chunks = [];
    let total = 0;
    for (;;) {
      const nextPromise = iterator.next();
      const next = signal
        ? await Promise.race([
            nextPromise,
            new Promise((_, reject) => {
              const abort = () => reject(abortError(signal));
              signal.addEventListener("abort", abort, { once: true });
              nextPromise.finally(() =>
                signal.removeEventListener("abort", abort)
              );
            })
          ])
        : await nextPromise;
      if (next.done) break;
      for (const chunk of normalizeChunk(next.value)) {
        total += chunk.byteLength;
        if (options.limit !== undefined && total > options.limit) {
          const error = new RangeError("stream exceeded the configured limit");
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
        chunks.push(chunk);
      }
    }
    return chunks;
  };
  const readSync = (source, options = {}) => {
    if (options.signal?.aborted) throw abortError(options.signal);
    const chunks = [];
    let total = 0;
    for (const value of sourceSync(source)) {
      for (const chunk of normalizeChunk(value)) {
        total += chunk.byteLength;
        if (options.limit !== undefined && total > options.limit) {
          const error = new RangeError("stream exceeded the configured limit");
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
        chunks.push(chunk);
      }
    }
    return chunks;
  };
  const readAsyncValues = async (source, options = {}) => {
    const values = [];
    let total = 0;
    if (options.signal?.aborted) throw abortError(options.signal);
    const iterator = sourceAsync(source)[Symbol.asyncIterator]();
    for (;;) {
      const nextPromise = iterator.next();
      const next = options.signal
        ? await Promise.race([
            nextPromise,
            new Promise((_, reject) => {
              const abort = () => reject(abortError(options.signal));
              options.signal.addEventListener("abort", abort, { once: true });
              nextPromise.finally(() =>
                options.signal.removeEventListener("abort", abort)
              );
            })
          ])
        : await nextPromise;
      if (next.done) break;
      const value = next.value;
      const batch = Array.isArray(value) ? value : [value];
      for (const item of batch) {
        total += item?.byteLength ?? NodeBuffer.byteLength(String(item));
        if (options.limit !== undefined && total > options.limit) {
          const error = new RangeError("stream exceeded the configured limit");
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
        values.push(item);
      }
    }
    return values;
  };
  const readSyncValues = (source, options = {}) => {
    const values = [];
    let total = 0;
    for (const value of sourceSync(source)) {
      const batch = Array.isArray(value) ? value : [value];
      for (const item of batch) {
        total += item?.byteLength ?? NodeBuffer.byteLength(String(item));
        if (options.limit !== undefined && total > options.limit) {
          const error = new RangeError("stream exceeded the configured limit");
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
        values.push(item);
      }
    }
    return values;
  };
  const concat = (chunks) => NodeBuffer.concat(chunks);
  const syncChunks = (source, options = {}) => {
    if (options.signal?.aborted) throw abortError(options.signal);
    const chunks = [];
    let total = 0;
    for (const value of source) {
      for (const chunk of normalizeChunk(value)) {
        total += chunk.byteLength;
        if (options.limit !== undefined && total > options.limit) {
          const error = new RangeError("stream exceeded the configured limit");
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
        chunks.push(chunk);
      }
    }
    return chunks;
  };
  const asyncBytes = async (source, options) =>
    concat(await readAsync(source, normalizeOptions(options)));
  const syncBytes = (source, options) =>
    concat(syncChunks(source, normalizeOptions(options)));
  const decoderEncoding = (options) => {
    const encoding = options.encoding || "utf-8";
    if (typeof encoding !== "string") {
      const error = new TypeError("encoding must be a string");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const normalized = encoding.toLowerCase();
    if (
      !"utf-8 utf8 utf-16le utf16le latin1 iso-8859-1 ascii"
        .split(" ")
        .includes(normalized)
    ) {
      const error = new RangeError(`Unknown encoding: ${encoding}`);
      error.code = "ERR_ENCODING_NOT_SUPPORTED";
      throw error;
    }
    return normalized === "latin1" ||
      normalized === "iso-8859-1" ||
      normalized === "ascii"
      ? "iso-8859-1"
      : normalized;
  };
  const decodeText = (bytes, encoding) => {
    if (encoding === "iso-8859-1") {
      let result = "";
      for (const byte of bytes) result += String.fromCharCode(byte);
      return result;
    }
    try {
      const result = new TextDecoder(encoding).decode(bytes);
      return encoding === "utf-8" && result.startsWith("\uFEFF")
        ? result.slice(1)
        : result;
    } catch (error) {
      const failure = new TypeError(error.message || "Invalid encoded data");
      failure.code = "ERR_INVALID_ARG_VALUE";
      throw failure;
    }
  };
  const asyncText = async (source, options) => {
    const opts = normalizeOptions(options);
    return decodeText(await asyncBytes(source, opts), decoderEncoding(opts));
  };
  const syncText = (source, options) => {
    const opts = normalizeOptions(options);
    return decodeText(syncBytes(source, opts), decoderEncoding(opts));
  };
  const asyncArray = async (source, options) =>
    readAsyncValues(source, normalizeOptions(options));
  const syncArray = (source, options) =>
    readSyncValues(source, normalizeOptions(options));
  const fromAsyncSource = (source) =>
    typeof source === "string"
      ? (async function* () {
          yield [NodeBuffer.from(source)];
        })()
      : sourceAsync(source);
  const fromSyncSource = (source) =>
    typeof source === "string"
      ? (function* () {
          yield [NodeBuffer.from(source)];
        })()
      : sourceSync(source);
  const tapSync = (observer) => {
    if (typeof observer !== "function") {
      throw new TypeError("observer must be a function");
    }
    return (value) => {
      observer(value);
      return value;
    };
  };
  const tap = (observer) => {
    if (typeof observer !== "function") {
      throw new TypeError("observer must be a function");
    }
    return async (value) => {
      await observer(value);
      return value;
    };
  };
  const pullSync = (readable, ...transforms) => {
    for (const transform of transforms) {
      if (
        typeof transform !== "function" &&
        typeof transform?.transform !== "function"
      ) {
        const error = new TypeError(
          "transform must be a function or an object with transform()"
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
    }
    return {
      *[Symbol.iterator]() {
        let current = sourceSync(readable);
        for (const transform of transforms) {
          if (typeof transform === "function") {
            const source = current;
            current = (function* () {
              for (const value of source) {
                const result = transform(value);
                if (result !== null && result !== undefined) yield result;
              }
              const flush = transform(null);
              if (flush !== null && flush !== undefined) yield flush;
            })();
          } else {
            const source = current;
            current = (function* () {
              const output = transform.transform(
                (function* () {
                  yield* source;
                  yield null;
                })()
              );
              yield* output;
            })();
          }
        }
        yield* current;
      }
    };
  };
  const push = () => {
    const queue = [];
    const waiters = [];
    let ended = false;
    const readable = {
      async *[Symbol.asyncIterator]() {
        for (;;) {
          if (queue.length) {
            yield queue.shift();
            continue;
          }
          if (ended) return;
          const value = await new Promise((resolve) => waiters.push(resolve));
          if (value === null) return;
          yield value;
        }
      }
    };
    const enqueue = (value) => {
      const batch = Array.isArray(value) ? value : [NodeBuffer.from(value)];
      const waiter = waiters.shift();
      if (waiter) waiter(batch);
      else queue.push(batch);
    };
    return {
      writer: {
        write(value) {
          if (ended) throw new Error("write after end");
          enqueue(value);
          return true;
        },
        end(value) {
          if (value !== undefined) enqueue(value);
          ended = true;
          while (waiters.length) waiters.shift()(null);
          return this;
        }
      },
      readable
    };
  };
  const merge = (...args) => {
    let options = {};
    if (
      args.length &&
      args[args.length - 1] &&
      typeof args[args.length - 1] === "object" &&
      !(args[args.length - 1] instanceof ArrayBuffer) &&
      !ArrayBuffer.isView(args[args.length - 1]) &&
      !args[args.length - 1][Symbol.iterator] &&
      !args[args.length - 1][Symbol.asyncIterator]
    ) {
      options = normalizeOptions(args.pop());
    }
    const sources = args.map((source) =>
      sourceAsync(source)[Symbol.asyncIterator]()
    );
    return {
      async *[Symbol.asyncIterator]() {
        const pending = sources.map((iterator, index) => ({
          index,
          iterator,
          promise: iterator.next().then((result) => ({ index, result }))
        }));
        let primaryError = null;
        try {
          while (pending.length) {
            if (options.signal?.aborted) throw abortError(options.signal);
            const next = Promise.race(pending.map((entry) => entry.promise));
            const result = options.signal
              ? await Promise.race([
                  next,
                  new Promise((_, reject) =>
                    options.signal.addEventListener(
                      "abort",
                      () => reject(abortError(options.signal)),
                      { once: true }
                    )
                  )
                ])
              : await next;
            const { index } = result;
            const entry = pending.find(
              (candidate) => candidate.index === index
            );
            if (!result.result.done) {
              yield result.result.value;
              entry.promise = entry.iterator
                .next()
                .then((nextResult) => ({ index, result: nextResult }));
            } else pending.splice(pending.indexOf(entry), 1);
          }
        } catch (error) {
          primaryError = error;
          throw error;
        } finally {
          try {
            await Promise.all(
              pending.map((entry) => entry.iterator.return?.())
            );
          } catch (cleanupError) {
            if (primaryError) {
              throw new SuppressedError(
                primaryError,
                cleanupError,
                "An error was suppressed during stream cleanup"
              );
            }
            throw cleanupError;
          }
        }
      }
    };
  };
  const broadcast = (options = {}) => {
    const consumers = new Set();
    const history = [];
    const budget = options.budget === undefined ? Infinity : options.budget;
    const backpressure = options.backpressure || "strict";
    const pendingWrites = [];
    const drainableProtocol = Symbol.for("Stream.drainableProtocol");
    let ended = false;
    let failure;
    let totalBytes = 0;
    const publish = (value) => {
      const batch = (Array.isArray(value) ? value : [value]).flatMap(
        normalizeChunk
      );
      const bytes = batch.reduce(
        (sum, item) =>
          sum + (item?.byteLength ?? NodeBuffer.byteLength(String(item))),
        0
      );
      if (totalBytes + bytes > budget) {
        if (backpressure === "drop-newest") return false;
        if (backpressure === "drop-oldest") {
          while (history.length && totalBytes + bytes > budget) {
            const dropped = history.shift();
            const droppedBytes = dropped.reduce(
              (sum, item) =>
                sum + (item?.byteLength ?? NodeBuffer.byteLength(String(item))),
              0
            );
            totalBytes -= droppedBytes;
            for (const consumer of consumers) {
              const index = consumer.queue.indexOf(dropped);
              if (index !== -1) consumer.queue.splice(index, 1);
            }
          }
        } else return false;
      }
      totalBytes += bytes;
      history.push(batch);
      for (const consumer of consumers) {
        const waiter = consumer.waiters.shift();
        if (waiter) waiter.resolve({ value: batch, done: false });
        else consumer.queue.push(batch);
      }
      return true;
    };
    const addConsumer = (options = {}) => {
      const consumer = {
        queue: history.slice(),
        waiters: [],
        done: ended,
        failure: undefined,
        signal: options?.signal,
        onAbort: null
      };
      const detachSignal = () => {
        if (consumer.signal && consumer.onAbort) {
          consumer.signal.removeEventListener("abort", consumer.onAbort);
        }
        consumer.onAbort = null;
      };
      const abort = () => {
        if (consumer.done) return;
        consumer.done = true;
        consumers.delete(consumer);
        const reason =
          consumer.signal.reason || new Error("The operation was aborted");
        consumer.failure = reason;
        while (consumer.waiters.length) consumer.waiters.shift().reject(reason);
        detachSignal();
      };
      if (consumer.signal) {
        consumer.onAbort = abort;
        if (consumer.signal.aborted) abort();
        else consumer.signal.addEventListener("abort", abort, { once: true });
      }
      consumers.add(consumer);
      return {
        async next() {
          if (consumer.queue.length) {
            const value = consumer.queue.shift();
            totalBytes = Math.max(
              0,
              totalBytes -
                value.reduce(
                  (sum, item) =>
                    sum +
                    (item?.byteLength ?? NodeBuffer.byteLength(String(item))),
                  0
                )
            );
            while (pendingWrites.length) {
              const pending = pendingWrites[0];
              const pendingBytes = (
                Array.isArray(pending.value) ? pending.value : [pending.value]
              ).reduce(
                (sum, item) =>
                  sum +
                  (item?.byteLength ?? NodeBuffer.byteLength(String(item))),
                0
              );
              if (totalBytes + pendingBytes > budget) break;
              pendingWrites.shift();
              if (pending.abort && pending.signal) {
                pending.signal.removeEventListener("abort", pending.abort);
              }
              if (publish(pending.value)) pending.resolve(true);
              else {
                pending.reject(
                  Object.assign(new RangeError("Invalid state: Failed"), {
                    code: "ERR_INVALID_STATE"
                  })
                );
              }
            }
            return { value, done: false };
          }
          if (consumer.done) {
            if (consumer.failure !== undefined) throw consumer.failure;
            if (failure !== undefined) throw failure;
            return { value: undefined, done: true };
          }
          return new Promise((resolve, reject) =>
            consumer.waiters.push({ resolve, reject })
          );
        },
        async return() {
          consumer.done = true;
          consumers.delete(consumer);
          while (consumer.waiters.length) {
            consumer.waiters.shift().resolve({ value: undefined, done: true });
          }
          detachSignal();
          return { value: undefined, done: true };
        },
        [Symbol.asyncIterator]() {
          return this;
        }
      };
    };
    const finish = (error) => {
      if (ended) return;
      ended = true;
      failure = error;
      while (pendingWrites.length) {
        const pending = pendingWrites.shift();
        pending.reject(
          error ||
            Object.assign(new TypeError("Invalid state: Failed"), {
              code: "ERR_INVALID_STATE"
            })
        );
      }
      for (const consumer of consumers) {
        if (error !== undefined) consumer.queue.length = 0;
        consumer.done = true;
        while (consumer.waiters.length) {
          const waiter = consumer.waiters.shift();
          if (error !== undefined) waiter.reject(error);
          else waiter.resolve({ value: undefined, done: true });
        }
        if (consumer.signal && consumer.onAbort) {
          consumer.signal.removeEventListener("abort", consumer.onAbort);
        }
        consumer.onAbort = null;
      }
      consumers.clear();
    };
    if (options.signal) {
      const abortBroadcast = () => finish(abortError(options.signal));
      if (options.signal.aborted) abortBroadcast();
      else {
        options.signal.addEventListener("abort", abortBroadcast, {
          once: true
        });
      }
    }
    const writer = {
      write(value, writeOptions) {
        if (ended) throw new Error("write after end");
        if (writeOptions?.signal?.aborted) {
          return Promise.reject(abortError(writeOptions.signal));
        }
        const bytes = (Array.isArray(value) ? value : [value]).reduce(
          (sum, item) =>
            sum + (item?.byteLength ?? NodeBuffer.byteLength(String(item))),
          0
        );
        if (totalBytes + bytes > budget && backpressure === "unbounded") {
          return new Promise((resolve, reject) => {
            const pending = {
              value,
              resolve,
              reject,
              signal: writeOptions?.signal
            };
            if (pending.signal) {
              pending.abort = () => {
                const index = pendingWrites.indexOf(pending);
                if (index !== -1) pendingWrites.splice(index, 1);
                reject(abortError(pending.signal));
              };
              pending.signal.addEventListener("abort", pending.abort, {
                once: true
              });
            }
            pendingWrites.push(pending);
          });
        }
        if (totalBytes + bytes > budget && backpressure === "strict") {
          const error = Object.assign(new RangeError("Invalid state: Failed"), {
            code: "ERR_INVALID_STATE"
          });
          if (pendingWrites.length) return Promise.reject(error);
          return new Promise((resolve, reject) =>
            pendingWrites.push({ value, resolve, reject })
          );
        }
        return publish(value);
      },
      writeSync(value) {
        if (ended) throw new Error("write after end");
        const bytes = (Array.isArray(value) ? value : [value]).reduce(
          (sum, item) =>
            sum + (item?.byteLength ?? NodeBuffer.byteLength(String(item))),
          0
        );
        if (
          totalBytes + bytes > budget &&
          (backpressure === "strict" || backpressure === "unbounded")
        ) {
          return false;
        }
        return publish(value);
      },
      writevSync(values) {
        return this.writeSync(values);
      },
      writev(values, options) {
        return this.write(values, options);
      },
      async end(value) {
        if (value?.signal?.aborted) {
          return Promise.reject(
            value.signal.reason || new Error("The operation was aborted")
          );
        }
        if (value !== undefined) publish(value);
        finish();
        return totalBytes;
      },
      endSync(value) {
        if (value !== undefined) publish(value);
        finish();
        return totalBytes;
      },
      fail(error) {
        finish(
          error ||
            Object.assign(new TypeError("Invalid state: Failed"), {
              code: "ERR_INVALID_STATE"
            })
        );
      },
      [drainableProtocol]() {
        return ended ? null : { desiredSize: budget - totalBytes };
      }
    };
    return {
      writer,
      broadcast: {
        push(options) {
          return addConsumer(options);
        },
        cancel(reason) {
          finish(reason);
        },
        get consumerCount() {
          return consumers.size;
        }
      }
    };
  };
  const shareSync = (source, options = {}) => {
    const sourceIterator = sourceSync(source)[Symbol.iterator]();
    const batches = [];
    const consumers = new Set();
    let cancelled = false;
    let sourceDone = false;
    let sourceError;
    const shared = {
      pull() {
        const consumer = { active: true, index: 0 };
        consumers.add(consumer);
        return {
          *[Symbol.iterator]() {
            try {
              while (!cancelled && consumer.active) {
                if (consumer.index < batches.length) {
                  yield batches[consumer.index++];
                  continue;
                }
                if (sourceError) throw sourceError;
                if (sourceDone) return;
                let result;
                try {
                  result = sourceIterator.next();
                } catch (error) {
                  sourceError = error;
                  throw error;
                }
                if (result.done) {
                  sourceDone = true;
                  return;
                }
                batches.push(result.value);
                consumer.index++;
                yield result.value;
              }
            } finally {
              consumer.active = false;
              consumers.delete(consumer);
            }
          }
        };
      },
      cancel() {
        cancelled = true;
        for (const consumer of consumers) consumer.active = false;
        consumers.clear();
        sourceIterator.return?.();
      },
      get consumerCount() {
        return consumers.size;
      }
    };
    return shared;
  };
  const validateReadableOptions = (options) => {
    if (options === undefined) return {};
    if (!options || typeof options !== "object" || Array.isArray(options)) {
      const error = new TypeError("options must be an object");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const highWaterMark = options.highWaterMark;
    if (highWaterMark !== undefined && typeof highWaterMark !== "number") {
      const error = new TypeError("highWaterMark must be a number");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (
      highWaterMark !== undefined &&
      (!Number.isInteger(highWaterMark) || highWaterMark < 0)
    ) {
      const error = new RangeError("highWaterMark must be non-negative");
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (
      options.signal !== undefined &&
      typeof options.signal?.addEventListener !== "function"
    ) {
      const error = new TypeError("signal must be an AbortSignal");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return options;
  };
  const validateReadableSource = (source) => {
    if (
      source === null ||
      source === undefined ||
      typeof source === "string" ||
      (!source?.[Symbol.iterator] && !source?.[Symbol.asyncIterator])
    ) {
      const error = new TypeError("source must be iterable");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
  };
  const toReadable = (source, options) => {
    const opts = validateReadableOptions(options);
    validateReadableSource(source);
    const chunks = (async function* () {
      for await (const value of sourceAsync(source)) {
        yield* normalizeChunk(value);
      }
    })();
    return globalThis.__nodeStream.Readable.from(chunks, {
      ...opts,
      objectMode: false,
      highWaterMark: opts.highWaterMark ?? 64 * 1024
    });
  };
  const toReadableSync = (source, options) => {
    const opts = validateReadableOptions(options);
    validateReadableSource(source);
    const chunks = (function* () {
      for (const value of sourceSync(source)) yield* normalizeChunk(value);
    })();
    return globalThis.__nodeStream.Readable.from(chunks, {
      ...opts,
      objectMode: false,
      highWaterMark: opts.highWaterMark ?? 64 * 1024
    });
  };
  const Broadcast = {
    from(source, options = {}) {
      const protocol = Symbol.for("Stream.broadcastProtocol");
      if (source && typeof source[protocol] === "function") {
        const result = source[protocol](options);
        if (!result || typeof result !== "object") {
          const error = new TypeError(
            "The broadcast protocol must return an object"
          );
          error.code = "ERR_INVALID_RETURN_VALUE";
          throw error;
        }
        return result;
      }
      const result = broadcast(options);
      const iterator = sourceAsync(source)[Symbol.asyncIterator]();
      const cancel = result.broadcast.cancel;
      result.broadcast.cancel = (reason) => {
        iterator.return?.();
        return cancel(reason);
      };
      (async () => {
        try {
          for (;;) {
            const next = await iterator.next();
            if (next.done) break;
            if (result.writer.write(next.value) === false) break;
          }
          result.writer.end();
        } catch (error) {
          result.writer.fail(error);
        }
      })();
      return result;
    }
  };
  const writableCache = new WeakMap();
  const fromWritable = (writable, options = {}) => {
    if (!writable || typeof writable.write !== "function") {
      const error = new TypeError("writable must be a Writable stream");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (writable.writableObjectMode) {
      const error = new Error("fromWritable does not support object mode");
      error.code = "ERR_INVALID_STATE";
      throw error;
    }
    if (options === null || typeof options !== "object") {
      const error = new TypeError("options must be an object");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (
      options.backpressure !== undefined &&
      !["wait", "drop-newest"].includes(options.backpressure)
    ) {
      const error = new TypeError("invalid backpressure option");
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    const state = writableCache.get(writable) || { ended: false };
    writableCache.set(writable, state);
    const backpressure = options.backpressure || state.backpressure || "wait";
    state.backpressure = backpressure;
    if (state.writer) return state.writer;
    const writer = {
      get canWrite() {
        return writable.destroyed ? null : !writable.writableNeedDrain;
      },
      async write(value) {
        if (state.ended) throw new Error("write after end");
        if (backpressure === "drop-newest" && state.blocked) return;
        const accepted = writable.write(value);
        state.blocked = !accepted;
        if (!accepted && backpressure === "wait") {
          await new Promise((resolve) => queueMicrotask(resolve));
        }
      },
      async end(value) {
        if (state.ended) return;
        if (value !== undefined) await this.write(value);
        state.ended = true;
        writable.end?.();
      },
      writev(chunks) {
        if (!Array.isArray(chunks)) {
          const error = new TypeError("chunks must be an array");
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        for (const chunk of chunks) {
          if (!(
            typeof chunk === "string" ||
            ArrayBuffer.isView(chunk) ||
            chunk instanceof ArrayBuffer
          )) {
            const error = new TypeError("chunk must be a string or buffer");
            error.code = "ERR_INVALID_ARG_TYPE";
            throw error;
          }
        }
        return Promise.all(chunks.map((chunk) => this.write(chunk)));
      },
      writeSync() {
        return false;
      },
      writevSync() {
        return false;
      },
      endSync() {
        return -1;
      }
    };
    state.writer = writer;
    return writer;
  };
  const toWritable = (writer) => {
    if (!writer || typeof writer.write !== "function") {
      const error = new TypeError("writer must provide write()");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const writable = new NodeWritable({
      write(chunk, encoding, callback) {
        if (typeof writer.writeSync === "function") {
          let accepted;
          try {
            accepted = writer.writeSync(chunk);
          } catch (error) {
            callback(error);
            return;
          }
          if (accepted === true) {
            queueMicrotask(callback);
            return;
          }
        }
        let pending;
        try {
          pending = writer.write(chunk);
        } catch (error) {
          callback(error);
          return;
        }
        Promise.resolve(pending).then(() => callback(), callback);
      },
      final(callback) {
        if (typeof writer.endSync === "function") {
          let result;
          try {
            result = writer.endSync();
          } catch (error) {
            callback(error);
            return;
          }
          if (result !== -1) {
            queueMicrotask(callback);
            return;
          }
        }
        let pending;
        try {
          pending = writer.end?.();
        } catch (error) {
          callback(error);
          return;
        }
        Promise.resolve(pending).then(() => callback(), callback);
      },
      destroy(error, callback) {
        if (error && typeof writer.fail === "function") {
          try {
            writer.fail(error);
          } catch (failure) {
            callback(failure);
            return;
          }
        }
        callback();
      }
    });
    writable._writev =
      typeof writer.writev === "function"
        ? (chunks, callback) =>
            Promise.resolve(
              writer.writev(chunks.map((item) => item.chunk))
            ).then(() => callback(), callback)
        : null;
    if (typeof writer.writev === "function") {
      const originalWrite = writable._write;
      writable._write = function (chunk, encoding, callback) {
        if (this.writableCorked > 0) {
          (this.__iterBatch ||= []).push({ chunk, callback });
          return;
        }
        originalWrite.call(this, chunk, encoding, callback);
      };
      const originalUncork = writable.uncork.bind(writable);
      writable.uncork = function () {
        originalUncork();
        const batch = this.__iterBatch?.splice(0) || [];
        if (!batch.length || this.writableCorked > 0) return;
        Promise.resolve(writer.writev(batch.map(({ chunk }) => chunk))).then(
          () => batch.forEach(({ callback }) => callback()),
          (error) => batch.forEach(({ callback }) => callback(error))
        );
      };
    }
    writable.writableHighWaterMark = Number.MAX_SAFE_INTEGER;
    return writable;
  };
  return {
    from: (source) =>
      typeof source?.[Symbol.asyncIterator] === "function"
        ? source
        : { [Symbol.asyncIterator]: () => fromAsyncSource(source) },
    fromSync: (source) => {
      if (
        source === null ||
        source === undefined ||
        (typeof source !== "string" &&
          !(source instanceof ArrayBuffer) &&
          !ArrayBuffer.isView(source) &&
          typeof source?.[toStreamable] !== "function" &&
          typeof source?.[Symbol.iterator] !== "function")
      ) {
        const error = new TypeError("input must be a synchronous streamable");
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      return { [Symbol.iterator]: () => fromSyncSource(source) };
    },
    text: asyncText,
    textSync: syncText,
    bytes: asyncBytes,
    bytesSync: syncBytes,
    arrayBuffer: async (source, options) =>
      (await asyncBytes(source, options)).buffer,
    arrayBufferSync: (source, options) => syncBytes(source, options).buffer,
    array: asyncArray,
    arraySync: syncArray,
    tap,
    tapSync,
    pullSync,
    shareSync,
    toReadable,
    toReadableSync,
    push,
    merge,
    broadcast,
    Broadcast,
    fromWritable,
    toWritable,
    ondrain: (writer) =>
      writer?.canWrite === null ? null : Promise.resolve(true),
    pull: (readable, transform) => {
      if (transform?.constructor?.name === "AsyncGeneratorFunction") {
        return {
          async *[Symbol.asyncIterator]() {
            yield* transform(sourceAsync(readable));
          }
        };
      }
      return {
        async *[Symbol.asyncIterator]() {
          for await (const value of readable) {
            const result = transform ? transform(value) : value;
            if (result && typeof result[Symbol.asyncIterator] === "function") {
              for await (const transformed of result) yield transformed;
            } else {
              yield result;
            }
          }
        }
      };
    },
    toStreamable,
    toAsyncStreamable
  };
};
const __quenchFinishClusterWorker = (cluster, worker, workerError) => {
  queueMicrotask(() => {
    if (worker.state !== "online" && worker.state !== "listening") return;
    const exitCode =
      process.exitCode !== undefined && process.exitCode !== 0
        ? process.exitCode
        : workerError
          ? 1
          : 0;
    worker.process.exitCode = exitCode;
    worker.process.signalCode = null;
    worker._markDead();
    cluster.emit("disconnect", worker);
    worker.emit("disconnect");
    cluster.emit("exit", worker, exitCode, null);
    worker.emit("exit", exitCode, null);
  });
};
const __quenchRunClusterWorker = (cluster, worker, env, reentry) => {
  if (typeof globalThis.__quench_script_source !== "string" || reentry) return;
  const previousAsyncResource = globalThis.__nodeCurrentAsyncResource;
  const previousIsWorker = cluster.isWorker;
  const previousArgv = process.argv;
  for (const [key, value] of Object.entries(env)) {
    if (value !== undefined) process.env[key] = value;
  }
  const setupArgs = cluster.settings?.args || [];
  if (setupArgs.length) process.argv = [...process.argv, ...setupArgs];
  cluster.isWorker = true;
  cluster.worker = worker;
  globalThis.__quench_in_cluster_worker = true;
  globalThis.__nodeCurrentAsyncResource = { id: worker.id };
  let workerError = null;
  try {
    (0, globalThis.eval)(globalThis.__quench_script_source);
  } catch (error) {
    workerError = error;
  }
  cluster.isWorker = previousIsWorker;
  process.argv = previousArgv;
  globalThis.__nodeCurrentAsyncResource = previousAsyncResource;
  if (worker.state !== "dead") {
    __quenchFinishClusterWorker(cluster, worker, workerError);
  }
};
const __quenchVmCopyProperties = (sandbox, keys, originalGlobalKeys) => {
  for (const key of keys) {
    const descriptor = Object.getOwnPropertyDescriptor(sandbox, key);
    if (descriptor && "value" in descriptor && descriptor.writable !== false) {
      sandbox[key] = globalThis[key];
    }
  }
  for (const key of Object.getOwnPropertyNames(globalThis)) {
    if (
      !originalGlobalKeys.has(key) &&
      !keys.includes(key) &&
      key !== "globalThis"
    ) {
      sandbox[key] = globalThis[key];
    }
  }
};
const __quenchVmIsObject = (v) =>
  v != null && ["object", "function"].includes(typeof v);
const __quenchVmRunCallback = (callback, sandbox, args) => {
  const state = __quenchVmInstallContext(sandbox);
  try {
    return callback(...args);
  } finally {
    __quenchVmCopyProperties(sandbox, state.keys, state.originalGlobalKeys);
    __quenchVmRestoreProperties(
      state.keys,
      state.previous,
      state.hiddenProcess,
      state.previousPrototype
    );
  }
};
const __quenchVmFormatError = (error, options, code) => {
  const match = /'([^']+)' is read-only/.exec(error.message || "");
  if (match) {
    error.message = `Cannot assign to read only property '${match[1]}'`;
  }
  const filename = typeof options === "string" ? options : options?.filename;
  if (filename) {
    const lineOffset =
      typeof options === "object" ? options.lineOffset || 0 : 0;
    const columnOffset =
      typeof options === "object" ? options.columnOffset || 0 : 0;
    error.stack = __quenchVmFormatStack(
      error,
      filename,
      lineOffset,
      columnOffset,
      code
    );
  }
};
const __quenchVmEvaluateContext = (code, sandbox, options, state) => {
  try {
    __quenchVmCheckRestrictedDeclaration(code);
    const source = String(code);
    const result =
      source.trim() === "window" &&
      Object.prototype.hasOwnProperty.call(sandbox, "window")
        ? globalThis
        : (0, eval)(source);
    __quenchVmCopyProperties(sandbox, state.keys, state.originalGlobalKeys);
    return result;
  } catch (error) {
    __quenchVmFormatError(
      error,
      options,
      state.formatCode ? String(code) : null
    );
    throw error;
  }
};
const __quenchVmValidateContext = (sandbox) => {
  if (!__quenchVmIsObject(sandbox)) {
    __quenchVmTypeError(
      'The "contextifiedObject" argument must be of type object.'
    );
  }
  if (!__quenchVmContexts.has(sandbox)) {
    __quenchVmTypeError(
      'The "contextifiedObject" argument must be an vm.Context'
    );
  }
};
const __quenchVmRunInContext = (code, sandbox, options) => {
  __quenchVmValidateContext(sandbox);
  const state = __quenchVmInstallContext(sandbox);
  try {
    return __quenchVmEvaluateContext(code, sandbox, options, state);
  } finally {
    __quenchVmRestoreProperties(
      state.keys,
      state.previous,
      state.hiddenProcess,
      state.previousPrototype
    );
  }
};
const __quenchVmModule = {
  Script: class Script {
    constructor(code, options) {
      __quenchVmValidateScriptOptions(options);
      this.code = String(code);
      this.sourceMapURL = __quenchVmSourceMapURL(this.code);
      __quenchVmApplyScriptCache(this, options);
    }
    createCachedData() {
      return NodeBuffer.from(this.code);
    }
    runInContext(context, options) {
      __quenchVmValidateScriptOptions(options);
      return __quenchVmRunInContext(this.code, context, options);
    }
    runInThisContext(options) {
      __quenchVmValidateScriptOptions(options);
      return (0, eval)(this.code);
    }
    runInNewContext(context = {}, options) {
      if (!(this instanceof __quenchVmModule.Script)) {
        throw new TypeError("this.runInContext is not a function");
      }
      __quenchVmValidateScriptOptions(options);
      return __quenchVmRunInNewContext(this.code, context, options);
    }
  },
  createScript: (code) => new __quenchVmModule.Script(code),
  compileFunction: (code, params, options) =>
    __quenchVmCompileFunction(code, params, options),
  SourceTextModule: class SourceTextModule {
    constructor() {
      this.namespace = Object.create(null);
      __nodeModuleNamespaces.add(this.namespace);
    }
    async link() {}
    async evaluate() {}
  },
  createContext: (sandbox = {}, options) => {
    if (!__quenchVmIsObject(sandbox)) {
      __quenchVmTypeError("The options argument must be an object");
    }
    __quenchVmValidateContextOptions(options);
    __quenchVmContexts.add(sandbox);
    return sandbox;
  },
  isContext: (value) => __quenchVmIsContext(value),
  runInThisContext: (code, options) => {
    try {
      return (0, eval)(String(code));
    } catch (error) {
      __quenchVmFormatError(error, options, null);
      throw error;
    }
  },
  runInNewContext: (code, sandbox = {}, options) =>
    __quenchVmRunInNewContext(code, sandbox, options),
  runInContext: (code, sandbox, options) =>
    __quenchVmRunInContext(code, sandbox, options)
};
const __quenchTrackTestResult = (result) => {
  if (!result?.then) return result;
  (globalThis.__quench_test_promises ||= []).push(result);
  result.catch((error) => {
    if (!globalThis.__quench_async_error) {
      globalThis.__quench_async_error = String(error?.stack || error);
    }
  });
  return result;
};
const __quenchNodeTestModule = (name, options, callback) =>
  __quenchTrackTestResult(
    (typeof options === "function" ? options : callback)({
      assert: globalThis.__nodeAssert,
      mock: {
        fn: (implementation = () => undefined) => {
          const wrapper = (...args) => {
            wrapper.mock._count++;
            wrapper.mock.calls.push(args);
            return implementation(...args);
          };
          wrapper.mock = {
            _count: 0,
            calls: [],
            callCount: () => wrapper.mock._count
          };
          return wrapper;
        }
      }
    })
  );
__quenchNodeTestModule.describe = (_name, callback) =>
  __quenchTrackTestResult(callback({ assert: globalThis.__nodeAssert }));
__quenchNodeTestModule.suite = __quenchNodeTestModule.describe;
__quenchNodeTestModule.it = __quenchNodeTestModule.describe;
__quenchNodeTestModule.test = __quenchNodeTestModule;
const __quenchDebugBinding = () => ({
  getGenericUsageCount: (name) =>
    name.includes("Uninitialized")
      ? __nodeAllocatorCounts.uninitialized
      : __nodeAllocatorCounts.zeroFilled
});
const __quenchInternalBindingModule = {
  internalBinding: (binding) => {
    const coreBinding = globalThis.__quenchInternalBindingCore(binding);
    if (coreBinding) return coreBinding;
    if (binding === "uv") {
      return {
        UV_ENOENT: -2,
        UV_EEXIST: -17,
        UV_UNKNOWN: -4094,
        errname: (errorNumber) =>
          globalThis.__nodeUtil.getSystemErrorName(errorNumber),
        getErrorMessage: (errorNumber) =>
          globalThis.__nodeUtil.getSystemErrorMessage(errorNumber)
      };
    }
    if (binding === "udp_wrap") {
      return { UDP: globalThis.__quenchDgramUDPClass };
    }
    if (binding === "timers") {
      return {
        getLibuvNow: () =>
          Number(BigInt(globalThis.__quench_now_ns()) / 1000000n)
      };
    }
    if (binding === "js_stream") {
      return {
        JSStream: class JSStream {
          constructor() {
            this._externalStream = { __quench_external: true };
          }
        }
      };
    }
    if (binding === "tcp_wrap") return globalThis.__quenchTcpBinding?.();
    if (binding === "util") {
      return {
        arrayBufferViewHasBuffer: (() => {
          const observed = new WeakSet();
          return (value) => {
            if (value.byteLength >= 96 || observed.has(value)) return true;
            observed.add(value);
            return false;
          };
        })(),
        previewEntries: () => []
      };
    }
    return globalThis.__quenchInternalFallbackBinding;
  }
};
const __quenchInternalErrorsModule = {
  codes: {
    ERR_OUT_OF_RANGE: class ERR_OUT_OF_RANGE extends RangeError {},
    ERR_IPC_CHANNEL_CLOSED: class ERR_IPC_CHANNEL_CLOSED extends Error {
      constructor() {
        super("Channel closed");
        this.code = "ERR_IPC_CHANNEL_CLOSED";
      }
    }
  }
};
const __quenchInternalBufferModule = {
  utf8Write: (buffer, string, offset = 0, length = buffer.length - offset) =>
    buffer.write(string, offset, length, "utf8")
};
Object.assign(__quenchInternalFsUtilsModule, {
  stringToFlags: (flags) => {
    const values = {
      r: 0,
      "r+": 2,
      rs: 1052674,
      "rs+": 1052674,
      sr: 1052674,
      "sr+": 1052674,
      w: 577,
      "w+": 578,
      wx: 705,
      xw: 705,
      "wx+": 706,
      "xw+": 706,
      a: 1089,
      "a+": 1090,
      ax: 1217,
      xa: 1217,
      "ax+": 1218,
      "xa+": 1218,
      as: 1053761,
      sa: 1053761,
      "as+": 1053762,
      "sa+": 1053762
    };
    if (typeof flags !== "string" || values[flags] === undefined) {
      const error = new TypeError(`Unknown file open flag: ${flags}`);
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    return values[flags];
  }
});
const __quenchRequireClusterInternal = (name) => {
  if (name === "internal/test/binding") return __quenchInternalBindingModule;
  if (name === "internal/errors") return __quenchInternalErrorsModule;
  if (name === "internal/buffer") return __quenchInternalBufferModule;
  if (name === "internal/fs/utils") return __quenchInternalFsUtilsModule;
  if (name === "zlib/iter") {
    return {
      compressGzip: () => (chunks) => chunks,
      decompressGzip: () => (chunks) => chunks
    };
  }
};
let __quenchClusterModule;
{
  if (!globalThis.__nodeCluster) {
    let forks = 0;
    class NodeClusterWorker extends globalThis.__nodeEventEmitter {
      constructor(id) {
        super();
        this.id = id;
        this.state = "none";
        this.exitedAfterDisconnect = false;
        const pid = 1000 + id;
        this.process = {
          pid,
          exitCode: undefined,
          signalCode: undefined,
          kill: (signal) => this.kill(signal)
        };
        this._sends = 0;
        const alive = globalThis.__quench_node_pids || new Set();
        globalThis.__quench_node_pids = alive;
        alive.add(pid);
      }
      send(...values) {
        const callback = values.at(-1);
        const hasCallback = typeof callback === "function";
        const message = hasCallback ? values.slice(0, -1) : values;
        const result = this._sends < 2;
        this._sends = hasCallback && this._sends === 3 ? 0 : this._sends + 1;
        queueMicrotask(() => {
          for (const value of message) this.emit("message", value);
        });
        if (hasCallback) {
          queueMicrotask(() => {
            this._sends = 0;
            callback(null);
          });
        }
        return result;
      }
      _markDead() {
        if (this.state === "dead") return;
        this.state = "dead";
        const alive = globalThis.__quench_node_pids;
        if (alive) alive.delete(this.process.pid);
      }
      kill(signal) {
        if (this.state === "dead") return this;
        const previousState = this.state;
        this.process.exitCode = null;
        this.process.signalCode = String(signal || "SIGTERM");
        if (previousState === "online" || previousState === "listening") {
          this.state = "disconnected";
        } else {
          this._markDead();
        }
        queueMicrotask(() => {
          if (previousState === "online" || previousState === "listening") {
            cluster.emit("disconnect", this);
            this.emit("disconnect");
            this._markDead();
            cluster.emit(
              "exit",
              this,
              this.process.exitCode,
              this.process.signalCode
            );
            this.emit("exit", this.process.exitCode, this.process.signalCode);
          } else {
            cluster.emit(
              "exit",
              this,
              this.process.exitCode,
              this.process.signalCode
            );
            this.emit("exit", this.process.exitCode, this.process.signalCode);
          }
        });
        return this;
      }
      disconnect() {
        if (this.state === "dead") return this;
        this.exitedAfterDisconnect = true;
        const previousState = this.state;
        this.process.exitCode = 0;
        this.process.signalCode = null;
        this._markDead();
        queueMicrotask(() => {
          cluster.emit("disconnect", this);
          this.emit("disconnect");
          if (previousState === "online" || previousState === "listening") {
            cluster.emit("exit", this, 0, null);
            this.emit("exit", 0, null);
          }
        });
        return this;
      }
    }
    const cluster = new globalThis.__nodeEventEmitter();
    cluster.isPrimary = true;
    cluster.isMaster = true;
    cluster.isWorker = false;
    cluster.settings = {};
    cluster.workers = [];
    cluster.Worker = NodeClusterWorker;
    cluster.setupPrimary = (settings = {}) => {
      cluster.settings = { ...settings };
      queueMicrotask(() => cluster.emit("setup"));
      return cluster.settings;
    };
    cluster.setupMaster = cluster.setupPrimary;
    cluster.fork = (env = {}) => {
      const worker = new NodeClusterWorker(++forks);
      cluster.workers.push(worker);
      worker._env = env;
      const reentry = globalThis.__quench_in_cluster_worker;
      queueMicrotask(() => {
        cluster.emit("fork", worker);
        if (worker.state !== "none") return;
        worker.state = "online";
        worker.emit("online");
        cluster.emit("online", worker);
        __quenchRunClusterWorker(cluster, worker, env, reentry);
      });
      return worker;
    };
    cluster.disconnect = (callback) => {
      for (const worker of cluster.workers) worker.disconnect();
      if (typeof callback === "function") queueMicrotask(callback);
      return cluster;
    };
    globalThis.__nodeCluster = cluster;
    globalThis.__nodeClusterListening = (info) => {
      const worker = cluster.worker;
      if (!worker) return;
      if (worker.state !== "online") return;
      worker.state = "listening";
      cluster.emit("listening", worker);
      worker.emit("listening", info);
    };
    __quenchClusterModule = cluster;
  }
}
globalThis.__quench_require_part_02 = (name, specifier) => {
  if (name === "cluster") {
    return globalThis.__nodeCluster || __quenchClusterModule;
  }
  if (name === "internal/event_target") {
    return {
      Event,
      EventTarget,
      CustomEvent,
      NodeEventTarget,
      kWeakHandler: Symbol("kWeakHandler")
    };
  }
  if (name === "stream") return globalThis.__nodeStream;
  if (name === "stream/iter") return __quenchRequireStreamIter();
  if (name === "vm") return __quenchVmModule;
  if (name === "worker_threads") {
    return { isMainThread: true, MessageChannel, MessagePort };
  }
  if (name === "node:test" || name === "test") return __quenchNodeTestModule;
  return __quenchRequireClusterInternal(name);
};
