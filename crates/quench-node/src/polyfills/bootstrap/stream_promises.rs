//! Polyfill: `stream-promises`

pub const JS: &str = r#"const __quenchOriginalRequireWithStreamPromises = globalThis.require;
const __quenchStreamPromises = {
  pipeline: (...streams) => {
    let options = {};
    const possibleOptions = streams[streams.length - 1];
    if (
      possibleOptions &&
      typeof possibleOptions === "object" &&
      !Array.isArray(possibleOptions) &&
      !possibleOptions.pipe &&
      !possibleOptions.write &&
      !possibleOptions.read
    ) {
      options = streams.pop();
    }
    return new Promise((resolve, reject) => {
      let settled = false;
      const complete = () => {
        if (!settled) {
          settled = true;
          resolve();
        }
      };
      const fail = (error) => {
        if (!settled) {
          settled = true;
          reject(error);
        }
      };
      const abort = () => {
        const error = Object.assign(new Error("The operation was aborted"), {
          name: "AbortError",
          code: "ABORT_ERR",
        });
        streams.at(-1)?.destroy?.(error);
        fail(error);
      };
      if (options.signal?.aborted) return abort();
      options.signal?.addEventListener?.("abort", abort, { once: true });
      queueMicrotask(() => {
        if (settled) return;
        globalThis.require("stream").pipeline(
          ...streams,
          (error) => error ? fail(error) : complete(),
        );
      });
    });
  },
  finished: (stream, options = {}) => {
    if (stream?._closedPromise) {
      return stream._closedPromise;
    }
    if (typeof stream?.getReader === "function") {
      return new Promise((resolve, reject) => {
        queueMicrotask(() => stream._error ? reject(stream._error) : resolve());
      });
    }
    if (typeof stream?.getWriter === "function") {
      if (stream._closed) return Promise.resolve();
      return new Promise((resolve, reject) => {
        stream._finishWaiters ||= [];
        stream._finishWaiters.push(() =>
          stream._error ? reject(stream._error) : resolve()
        );
      });
    }
    if (options.cleanup !== undefined && typeof options.cleanup !== "boolean") {
      throw Object.assign(
        new TypeError('The "cleanup" option must be of type boolean'),
        {
          code: "ERR_INVALID_ARG_TYPE",
        },
      );
    }
    return new Promise((resolve, reject) => {
      const readable = options.readable !== false &&
        stream.readable !== false &&
        (stream.readable !== undefined || stream.readableEnded !== undefined);
      const writable = options.writable !== false &&
        stream.writable !== false &&
        (stream.writable !== undefined || stream.writableEnded !== undefined);
      let ended = !readable;
      let finished = !writable;
      let settled = false;
      const onEnd = () => {
        if (readable) {
          ended = true;
          settle();
        }
      };
      const onFinish = () => {
        if (writable) {
          finished = true;
          settle();
        }
      };
      const onError = (error) => reject(error);
      const settle = () => {
        if (!settled && ended && finished) {
          settled = true;
          resolve();
          if (options.cleanup === true) {
            stream.removeListener?.("end", onEnd);
            stream.removeListener?.("finish", onFinish);
            stream.removeListener?.("error", onError);
          }
        }
      };
      stream.once("end", onEnd);
      if (writable) stream.once("finish", onFinish);
      stream.once("error", onError);
      settle();
    });
  },
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "stream/promises") {
    return __quenchStreamPromises;
  }
  return __quenchOriginalRequireWithStreamPromises(specifier);
};
"#;
