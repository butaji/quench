const __quenchFetchAbortError = (reason) => {
  const error = new Error("The operation was aborted");
  error.name = "AbortError";
  error.code = "ABORT_ERR";
  error.cause = reason;
  return error;
};

Object.defineProperty(globalThis, "fetch", {
  value: (input, init = {}) => {
    const request = new globalThis.Request(input, init);
    const signal = init.signal || request.signal;
    if (signal?.aborted)
      return Promise.reject(__quenchFetchAbortError(signal.reason));
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
      const cleanup = () => signal?.removeEventListener?.("abort", onAbort);
      const onAbort = () => {
        requestHandle?.destroy?.(__quenchFetchAbortError(signal.reason));
        cleanup();
        reject(__quenchFetchAbortError(signal.reason));
      };
      let requestHandle;
      try {
        requestHandle = http.request(options, (response) => {
          const chunks = [];
          response.on("data", (chunk) => chunks.push(chunk));
          response.once("end", () => {
            cleanup();
            const body = chunks.map((chunk) => String(chunk)).join("");
            resolve(
              new globalThis.Response(body, {
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
        });
        requestHandle.once("error", (error) => {
          cleanup();
          reject(error);
        });
        signal?.addEventListener?.("abort", onAbort, { once: true });
        if (request.body != null) requestHandle.write(request.body);
        requestHandle.end();
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
