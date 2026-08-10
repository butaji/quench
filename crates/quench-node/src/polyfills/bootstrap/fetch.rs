//! Polyfill: `fetch`

pub const JS: &str = r#"const __quenchFetchAbortError = (reason) => {
  const error = new Error("The operation was aborted");
  error.name = "AbortError";
  error.code = "ABORT_ERR";
  error.cause = reason;
  return error;
};
const __quenchFetchResponse = (response, resolve, reject, cleanup) => {
  const chunks = [];
  response.on("data", (chunk) => chunks.push(chunk));
  response.once("end", () => {
    cleanup();
    resolve(
      new globalThis.Response(chunks.map(String).join(""), {
        status: response.statusCode,
        statusText: response.statusMessage,
        headers: response.headers
      })
    );
  });
  response.once("error", (error) => {
    cleanup();
    reject(error);
  });
};
const __quenchFetchSend = (http, options, request, signal, cleanup, reject) => {
  let requestHandle;
  const onAbort = () => {
    requestHandle?.destroy?.(__quenchFetchAbortError(signal.reason));
    cleanup();
    reject(__quenchFetchAbortError(signal.reason));
  };
  requestHandle = http.request(options, (response) =>
    __quenchFetchResponse(response, options.resolve, reject, cleanup)
  );
  requestHandle.once("error", (error) => {
    cleanup();
    reject(error);
  });
  signal?.addEventListener?.("abort", onAbort, { once: true });
  if (request.body != null) requestHandle.write(request.body);
  requestHandle.end();
};
Object.defineProperty(globalThis, "fetch", {
  value: (input, init = {}) => {
    const request = new globalThis.Request(input, init);
    const signal = init.signal || request.signal;
    if (signal?.aborted) {
      return Promise.reject(__quenchFetchAbortError(signal.reason));
    }
    return new Promise((resolve, reject) => {
      const target = new URL(request.url);
      const http = globalThis.__nodeHttp || globalThis.require("http");
      const options = {
        protocol: target.protocol,
        hostname: target.hostname,
        port: target.port || undefined,
        path: `${target.pathname || "/"}${target.search}`,
        method: request.method,
        headers: Object.fromEntries(request.headers)
      };
      const cleanup = () => {};
      options.resolve = resolve;
      try {
        __quenchFetchSend(http, options, request, signal, cleanup, reject);
      } catch (error) {
        cleanup();
        reject(error);
      }
    });
  },
  configurable: true,
  writable: true,
  enumerable: false
});
"#;
