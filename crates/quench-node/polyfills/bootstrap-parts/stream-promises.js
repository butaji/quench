const __quenchOriginalRequireWithStreamPromises = globalThis.require;
const __quenchStreamPromises = {
  pipeline: (...streams) => {
    const destination = streams.pop();
    for (const source of streams) source.pipe(destination);
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
      const readable =
        destination.readable !== false &&
        (destination.readable !== undefined ||
          destination.readableEnded !== undefined);
      const writable =
        destination.writable !== false &&
        (destination.writable !== undefined ||
          destination.writableEnded !== undefined);
      if (readable) destination.once("end", complete);
      if (writable) destination.once("finish", complete);
      for (const stream of streams) stream.once?.("error", fail);
      destination.once?.("error", fail);
      for (const stream of streams) {
        stream.once?.("close", () => {
          if (stream.readable && !stream.readableEnded) {
            const error = new Error("Premature close");
            error.code = "ERR_STREAM_PREMATURE_CLOSE";
            fail(error);
          }
        });
      }
    });
  },
  finished: (stream, options = {}) => {
    if (typeof stream?.getReader === "function") {
      return new Promise((resolve, reject) => {
        queueMicrotask(() =>
          stream._error ? reject(stream._error) : resolve()
        );
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
          code: "ERR_INVALID_ARG_TYPE"
        }
      );
    }
    return new Promise((resolve, reject) => {
      const readable =
        options.readable !== false &&
        stream.readable !== false &&
        (stream.readable !== undefined || stream.readableEnded !== undefined);
      const writable =
        options.writable !== false &&
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
  }
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "stream/promises") {
    return __quenchStreamPromises;
  }
  return __quenchOriginalRequireWithStreamPromises(specifier);
};
