//! Polyfill: `network-promises-tail`

pub const JS: &str = quench_js_check::checked_js!(r#"globalThis.__quenchNetPromisesModule = {
  listen(options = {}) {
    if (options === null || typeof options !== "object") {
      const error = new TypeError(
        'The "options" argument must be of type object',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      return Promise.reject(error);
    }
    const { signal, connectionListener, ...listenOptions } = options;
    if (
      signal !== undefined &&
      (signal === null || typeof signal !== "object" ||
        typeof signal.aborted !== "boolean")
    ) {
      const error = new TypeError(
        'The "options.signal" property must be an instance of AbortSignal',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      return Promise.reject(error);
    }
    if (signal?.aborted) {
      const error = new Error("The operation was aborted");
      error.name = "AbortError";
      error.code = "ABORT_ERR";
      return Promise.reject(error);
    }
    return new Promise((resolve, reject) => {
      const server = __quenchNetModule.createServer(
        listenOptions,
        connectionListener,
      );
      let listening = false;
      let settled = false;
      const onAbort = () => {
        if (listening) {
          server.close();
          return;
        }
        settled = true;
        server.close?.();
        const error = new Error("The operation was aborted");
        error.name = "AbortError";
        error.code = "ABORT_ERR";
        reject(error);
      };
      const onError = (error) => {
        if (settled) return;
        settled = true;
        server.removeListener("listening", onListening);
        reject(error);
      };
      const onListening = () => {
        if (settled) return;
        listening = true;
        settled = true;
        server.removeListener("error", onError);
        if (signal) signal.addEventListener("abort", onAbort, { once: true });
        resolve(server);
      };
      if (signal) signal.addEventListener("abort", onAbort, { once: true });
      server.once("error", onError);
      server.once("listening", onListening);
      server.listen(listenOptions);
    });
  },
};
"#);
