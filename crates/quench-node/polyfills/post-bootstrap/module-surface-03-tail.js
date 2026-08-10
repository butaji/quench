const __quenchAddZlibValidation = (result) => {
  for (const name of "gzip gunzip deflate inflate gzipSync gunzipSync deflateSync inflateSync".split(
    " "
  )) {
    if (typeof result[name] !== "function") continue;
    const original = result[name];
    result[name] = (value, options, callback) => {
      __quenchValidateZlibOptions(
        options,
        name === "gunzip" ||
          name === "inflate" ||
          name === "gunzipSync" ||
          name === "inflateSync"
      );
      return original(value, options, callback);
    };
  }
  if (result.codes) {
    Object.defineProperty(result, "codes", {
      value: result.codes,
      enumerable: true,
      writable: false,
      configurable: true
    });
  }
  return result;
};
const __quenchReadableAbortError = (signal) =>
  signal?.reason ||
  Object.assign(new Error("The operation was aborted"), {
    name: "AbortError",
    code: "ABORT_ERR"
  });
const __quenchReadableOperatorOptions = (options) => {
  const value = options === undefined ? {} : options;
  __quenchSliceOptions(value);
  const concurrency =
    value.concurrency === undefined ? 1 : Number(value.concurrency);
  const highWaterMark =
    value.highWaterMark === undefined
      ? concurrency - 1
      : Number(value.highWaterMark);
  if (!Number.isInteger(concurrency) || concurrency < 1) {
    const error = new RangeError(
      "ERR_OUT_OF_RANGE: concurrency must be positive"
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  if (!Number.isInteger(highWaterMark) || highWaterMark < 0) {
    const error = new RangeError(
      "ERR_OUT_OF_RANGE: highWaterMark must be non-negative"
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  return { ...value, concurrency, highWaterMark };
};
const __quenchReadableOperatorResult = (promise, errorState) => {
  if (!errorState) return promise;
  if (errorState.hasError) return Promise.reject(errorState.error);
  return new Promise((resolve, reject) => {
    const cleanup = () => errorState.listeners.delete(onError);
    const onError = (error) => {
      cleanup();
      reject(error);
    };
    errorState.listeners.add(onError);
    Promise.resolve(promise).then(
      (value) => {
        cleanup();
        resolve(value);
      },
      (error) => {
        cleanup();
        reject(error);
      }
    );
  });
};
const __quenchReadableConcurrentTransform = async function* (
  source,
  callback,
  options,
  mode,
  errorState,
  prefetchBeforeYield = true
) {
  const opts = __quenchReadableOperatorOptions(options);
  const controller = new AbortController();
  const signal = controller.signal;
  const externalSignal = opts.signal;
  const iterator = source[Symbol.asyncIterator]();
  const pending = [];
  const queueLimit = opts.concurrency + opts.highWaterMark;
  let active = 0;
  let resumeProgress;
  let sourceDone = false;
  const notifyProgress = () => {
    if (!resumeProgress) return;
    const resume = resumeProgress;
    resumeProgress = undefined;
    resume();
  };
  const onAbort = () => {
    controller.abort(externalSignal.reason);
    notifyProgress();
  };
  if (externalSignal?.aborted) onAbort();
  else externalSignal?.addEventListener?.("abort", onAbort, { once: true });
  const fill = async () => {
    while (
      !sourceDone &&
      active < opts.concurrency &&
      pending.length < queueLimit
    ) {
      if (signal.aborted) throw __quenchReadableAbortError(signal);
      const next = await iterator.next();
      if (signal.aborted) throw __quenchReadableAbortError(signal);
      if (next.done) {
        sourceDone = true;
        break;
      }
      const entry = {
        error: undefined,
        failed: false,
        result: undefined,
        settled: false,
        value: next.value
      };
      active++;
      Promise.resolve()
        .then(() => callback(next.value, { signal }))
        .then(
          (result) => {
            entry.result = result;
            entry.settled = true;
            active--;
            notifyProgress();
            return entry;
          },
          (error) => {
            entry.error = error;
            entry.failed = true;
            entry.settled = true;
            active--;
            notifyProgress();
            return entry;
          }
        );
      pending.push(entry);
    }
  };
  const waitForProgress = () =>
    new Promise((resolve) => {
      resumeProgress = resolve;
    });
  try {
    await fill();
    while (pending.length) {
      const entry = pending[0];
      while (!entry.settled) {
        if (signal.aborted) throw __quenchReadableAbortError(signal);
        await __quenchReadableOperatorResult(waitForProgress(), errorState);
        if (signal.aborted) throw __quenchReadableAbortError(signal);
        await fill();
      }
      if (errorState?.hasError) throw errorState.error;
      pending.shift();
      if (entry.failed) throw entry.error;
      if (signal.aborted) throw __quenchReadableAbortError(signal);
      const result = entry.result;
      if (prefetchBeforeYield || mode !== "filter" || !result) await fill();
      if (mode === "filter") {
        if (result) {
          yield entry.value;
          if (!prefetchBeforeYield) await fill();
        }
      } else if (mode !== "forEach") {
        if (mode === "flatMap" && result != null) {
          if (Array.isArray(result)) yield* result;
          else if (result?.toArray) yield* await result.toArray();
          else yield result;
        } else {
          yield result;
        }
      }
    }
  } finally {
    controller.abort();
    externalSignal?.removeEventListener?.("abort", onAbort);
    iterator.return?.();
  }
};
const __quenchReadableSliceSource = async function* (stream, operations) {
  const iterator = stream[Symbol.asyncIterator]();
  const signal = operations.signal;
  let skipped = operations.drop;
  let remaining = operations.take;
  const next = () => {
    if (!signal) return iterator.next();
    return new Promise((resolve, reject) => {
      let settled = false;
      const cleanup = () => signal.removeEventListener?.("abort", onAbort);
      const onAbort = () => {
        if (settled) return;
        settled = true;
        cleanup();
        reject(__quenchReadableAbortError(signal));
      };
      signal.addEventListener?.("abort", onAbort, { once: true });
      if (signal.aborted) {
        onAbort();
        return;
      }
      let pending;
      try {
        pending = iterator.next();
      } catch (error) {
        settled = true;
        cleanup();
        reject(error);
        return;
      }
      Promise.resolve(pending).then(
        (value) => {
          if (settled) return;
          settled = true;
          cleanup();
          resolve(value);
        },
        (error) => {
          if (settled) return;
          settled = true;
          cleanup();
          reject(error);
        }
      );
    });
  };
  try {
    if (signal?.aborted) throw __quenchReadableAbortError(signal);
    while (remaining > 0) {
      const item = await next();
      if (item.done) return;
      if (skipped > 0) {
        skipped--;
        continue;
      }
      remaining--;
      yield item.value;
    }
  } finally {
    iterator.return?.();
    if (!stream.readableEnded && !stream.destroyed) stream.destroy?.();
  }
};
const __quenchReadableSliceValues = (stream, operations) => {
  let current = __quenchReadableSliceSource(stream, operations);
  for (const operation of operations.operations) {
    current = __quenchReadableConcurrentTransform(
      current,
      operation.callback,
      operation.options,
      operation.type,
      operations.errorState
    );
  }
  return current;
};
const __quenchCollectReadable = async (stream, operations) => {
  const values = [];
  for await (const value of __quenchReadableSliceValues(stream, operations)) {
    values.push(value);
  }
  return values;
};
const __quenchReadableAbortableResult = (promise, signal) => {
  if (!signal) return promise;
  if (signal.aborted) {
    return Promise.reject(__quenchReadableAbortError(signal));
  }
  return new Promise((resolve, reject) => {
    let settled = false;
    const cleanup = () => signal.removeEventListener?.("abort", onAbort);
    const onAbort = () => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(__quenchReadableAbortError(signal));
    };
    signal.addEventListener?.("abort", onAbort, { once: true });
    Promise.resolve(promise).then(
      (value) => {
        if (settled) return;
        settled = true;
        cleanup();
        resolve(value);
      },
      (error) => {
        if (settled) return;
        settled = true;
        cleanup();
        reject(error);
      }
    );
  });
};
const __quenchReduceReadable = async (
  readable,
  reducer,
  initialValue,
  options,
  hasInitialValue
) => {
  if (typeof reducer !== "function") {
    const error = new TypeError(
      "ERR_INVALID_ARG_TYPE: reducer must be a function"
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (options !== undefined && options !== null) {
    if (typeof options !== "object") {
      const error = new TypeError(
        "ERR_INVALID_ARG_TYPE: options must be an object"
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (
      options.signal !== undefined &&
      (typeof options.signal !== "object" ||
        typeof options.signal.aborted !== "boolean" ||
        typeof options.signal.addEventListener !== "function")
    ) {
      const error = new TypeError(
        "ERR_INVALID_ARG_TYPE: signal must be an AbortSignal"
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
  }
  const externalSignal = options?.signal;
  if (externalSignal?.aborted) {
    const error = __quenchReadableAbortError(externalSignal);
    readable.once?.("error", () => {});
    readable.destroy?.(error);
    throw error;
  }
  const controller = new AbortController();
  const onAbort = () => controller.abort(externalSignal.reason);
  externalSignal?.addEventListener?.("abort", onAbort, { once: true });
  let gotValue = false;
  try {
    for await (const value of readable) {
      gotValue = true;
      if (externalSignal?.aborted) {
        throw __quenchReadableAbortError(externalSignal);
      }
      if (!hasInitialValue) {
        initialValue = value;
        hasInitialValue = true;
      } else {
        initialValue = await __quenchReadableAbortableResult(
          reducer(initialValue, value, { signal: controller.signal }),
          externalSignal
        );
      }
    }
    if (!gotValue && !hasInitialValue) {
      const error = new TypeError(
        "Reduce of an empty stream requires an initial value"
      );
      error.code = "ERR_MISSING_ARGS";
      throw error;
    }
    return initialValue;
  } finally {
    controller.abort();
    externalSignal?.removeEventListener?.("abort", onAbort);
  }
};
const __quenchPredicateReadable = async (
  readable,
  predicate,
  options,
  mode
) => {
  __quenchIterableOptions(predicate, options ?? undefined);
  const slice = __quenchReadableSlice(readable, {
    drop: 0,
    take: Infinity,
    operations: []
  });
  const callback =
    mode === "every"
      ? async (...args) => !(await predicate(...args))
      : predicate;
  const values = __quenchReadableConcurrentTransform(
    slice[Symbol.asyncIterator](),
    callback,
    options ?? undefined,
    "filter",
    undefined,
    false
  );
  for await (const value of values) {
    readable.destroy?.();
    if (mode === "find") return value;
    return mode === "some";
  }
  if (mode === "every") return true;
  if (mode === "some") return false;
  return undefined;
};
const __quenchSliceCount = (count) => {
  let value = Number(count);
  if (Number.isNaN(value)) value = 0;
  if (!Number.isFinite(value) || value < 0) {
    const error = new RangeError(
      "ERR_OUT_OF_RANGE: count must be non-negative"
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  return Math.floor(value);
};
const __quenchSliceOptions = (options) => {
  if (options === undefined) return;
  if (!options || typeof options !== "object") {
    const error = new TypeError(
      "ERR_INVALID_ARG_TYPE: options must be an object"
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (options.signal !== undefined && typeof options.signal !== "object") {
    const error = new TypeError(
      "ERR_INVALID_ARG_TYPE: signal must be an AbortSignal"
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
const __quenchValidateZlibOptions = (options, allowZeroWindowBits = false) => {
  const value = options?.windowBits;
  if (
    value !== undefined &&
    (!Number.isInteger(value) ||
      value > 15 ||
      (value < 9 && !(allowZeroWindowBits && value === 0)))
  ) {
    const error = new RangeError(
      `The value of "options.windowBits" is out of range. It must be >= 9 and <= 15. Received ${value}`
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
};
