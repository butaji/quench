//! Polyfill: `events-head`

pub const JS: &str = quench_js_check::checked_js!(r#"for (const method of "on addListener once emit removeListener off removeAllListeners listeners listenerCount".split(
  " "
)) {
  globalThis.process[method] = NodeEventEmitter.prototype[method];
}
const __nodeWritableWriteError = (stream, callback, error) => {
  if (!callback && stream.__writeErrorEmitted) return false;
  if (!callback) stream.__writeErrorEmitted = true;
  queueMicrotask(() => {
    if (callback) callback(error);
    else stream.emit("error", error);
  });
  return false;
};
const __nodeWritableDestroyComplete = (stream, callback, error) => {
  if (stream.__destroyCompleteScheduled) return;
  stream.__destroyCompleteScheduled = true;
  if (error) {
    stream._writableState.errored = error;
    stream.errored = error;
  }
  queueMicrotask(() => {
    if (error) {
      stream._writableState.errorEmitted = true;
      if (!callback) stream.emit("error", error);
    }
    stream.closed = true;
    stream.emit("close");
    if (callback) callback(error);
  });
};
globalThis.__nodeEventEmitter.once = (emitter, event) =>
  new Promise((resolve) => {
    if (typeof emitter.once === "function") {
      return emitter.once(event, (...args) => resolve(args));
    }
    if (typeof emitter.addEventListener === "function") {
      const listener = (...args) => {
        emitter.removeEventListener?.(event, listener);
        resolve(args);
      };
      emitter.addEventListener(event, listener, { once: true });
    }
  });
globalThis.__nodeEventEmitter.on = async function* (emitter, event, options) {
  const queue = [];
  let wake;
  let aborted = false;
  const listener = (...args) => {
    if (aborted) return;
    queue.push(args);
    if (wake) (wake(), (wake = undefined));
  };
  emitter.on(event, listener);
  const signal = options?.signal;
  if (signal?.aborted) {
    throw new DOMException("The operation was aborted.", "AbortError");
  }
  const onAbort = () => {
    aborted = true;
    if (wake) (wake(), (wake = undefined));
  };
  signal?.addEventListener("abort", onAbort);
  try {
    while (true) {
      if (aborted) {
        throw new DOMException("The operation was aborted.", "AbortError");
      }
      if (!queue.length) await new Promise((resolve) => (wake = resolve));
      yield queue.shift();
    }
  } finally {
    signal?.removeEventListener("abort", onAbort);
    emitter.off(event, listener);
  }
};
const __nodeReadableEmitClose = (stream) => {
  if (stream._closeEmitted) return;
  stream._closeEmitted = true;
  stream.closed = true;
  stream.emit("close");
};
const __nodeHasUserErrorListener = (stream) =>
  stream.listeners("error").some((listener) => !listener.__quenchInternal);
const __nodeReadableValidateChunk = (stream, chunk) => {
  if (
    stream.readableObjectMode ||
    typeof chunk === "string" ||
    ArrayBuffer.isView(chunk)
  ) {
    return true;
  }
  const error = new TypeError("chunk must be a string or buffer");
  error.code = "ERR_INVALID_ARG_TYPE";
  stream.emit("error", error);
  return false;
};
const __nodeReadablePushEnd = (stream) => {
  stream._readableState.reading = false;
  stream._readableState.ended = true;
  stream._readableState.readingMore = false;
  stream._readableState.needReadable = false;
  stream._ended = true;
  if (!stream._chunks.length) {
    if (stream.listenerCount("readable")) {
      if (!stream._readableState.dataEmitted) {
        __nodeReadableScheduleReadable(stream);
      }
      queueMicrotask(() => stream._emitEnd());
    } else if (stream.listenerCount("data")) {
      queueMicrotask(() => stream._emitEnd());
    } else stream._emitEnd();
  }
  return false;
};
const __nodeReadableScheduleReadable = (stream) => {
  if (stream._readableEventPending) return;
  stream._readableEventPending = true;
  process.nextTick(() => {
    stream._readableEventPending = false;
    if (stream._chunks.length || stream._readableState.ended) {
      stream._readableState.emittedReadable = true;
      stream._readableState.needReadable = false;
      stream.emit("readable");
    }
  });
};
const __nodeWritableComplete = (state, stream, size, callback, error) => {
  if (state.completed) {
    const duplicate = new Error("Callback called multiple times");
    duplicate.code = "ERR_MULTIPLE_CALLBACK";
    queueMicrotask(() => stream.emit("error", duplicate));
    return;
  }
  state.completed = true;
  stream._writableState.writing = false;
  if (error) stream.emit("error", error);
  stream.writableLength = Math.max(0, stream.writableLength - size);
  if (
    stream.writableNeedDrain &&
    (stream.writableLength === 0 ||
      stream.writableLength < stream.writableHighWaterMark)
  ) {
    stream.writableNeedDrain = false;
    stream._writableState.needDrain = false;
    stream.emit("drain");
  }
  if (callback) callback(error);
  if (!stream.destroyed && stream._writeQueue?.length) {
    const next = stream._writeQueue.shift();
    stream.__nodeProcessWrite(next);
  }
  stream.__nodeMaybeFinish?.();
};
const __nodeReadableClearAwaitDrain = (stream) => {
  if (stream._readableState.awaitDrainWriters instanceof Set) {
    stream._readableState.awaitDrainWriters.clear();
  } else {
    stream._readableState.awaitDrainWriters = null;
  }
};
const __nodeReadablePushChunk = (stream, chunk) => {
  stream._readableState.reading = false;
  stream._readableState.readingMore = true;
  if (
    stream._readableState.sync ||
    stream._chunks.length > 0 ||
    stream._paused ||
    (stream.listenerCount("data") === 0 && stream.readableFlowing !== true)
  ) {
    stream._chunks.push(chunk);
    if (stream.listenerCount("readable")) {
      __nodeReadableScheduleReadable(stream);
    }
  } else {
    __nodeReadableClearAwaitDrain(stream);
    stream._readableState.dataEmitted = true;
    stream._readableState.needReadable = false;
    if (stream.listenerCount("data")) {
      stream.emit("data", stream._decode(chunk));
    }
  }
  const length = chunk?.byteLength ?? chunk?.length ?? 0;
  if (length < stream.readableHighWaterMark) {
    queueMicrotask(() => __nodeReadableStart(stream));
  }
  return true;
};
const __nodeReadableStart = (stream) => {
  if (
    stream._ended ||
    stream.destroyed ||
    stream._readableState.errored ||
    stream._readableState.reading ||
    (!stream.readableObjectMode &&
      stream.readableHighWaterMark > 0 &&
      stream.readableLength >= stream.readableHighWaterMark)
  ) {
    return;
  }
  stream._readableState.reading = true;
  stream._readableState.sync = true;
  try {
    stream._read?.call(stream);
    stream._readableState.sync = false;
  } catch (error) {
    stream._readableState.sync = false;
    stream._readableState.reading = false;
    stream._readableState.errored = error;
    stream.errored = error;
    queueMicrotask(() => {
      stream._readableState.errorEmitted = true;
      stream.emit("error", error);
    });
  }
};
const __nodeReadableReadSized = (stream, chunk, size) => {
  if (size === undefined || stream.readableObjectMode || !chunk) {
    return undefined;
  }
  if (chunk.length < size) {
    const parts = [chunk];
    let length = chunk.length;
    while (stream._chunks.length && length < size) {
      const next = stream._chunks.shift();
      const remaining = size - length;
      if (next.length > remaining) {
        parts.push(next.subarray(0, remaining));
        stream._chunks.unshift(next.subarray(remaining));
        length = size;
      } else {
        parts.push(next);
        length += next.length;
      }
    }
    if (parts.length > 1) return stream._decode(NodeBuffer.concat(parts));
  }
  if (chunk.length > size) {
    stream._chunks.unshift(chunk.subarray(size));
    return stream._decode(chunk.subarray(0, size));
  }
  return undefined;
};
const __nodeReadableFinishRead = (stream, chunk) => {
  if (!stream._chunks.length && stream._ended) stream._emitEnd();
  const result = stream._decode(chunk);
  if (!result?.byteLength && !stream._ended) __nodeReadableStart(stream);
  return result;
};
"#);
