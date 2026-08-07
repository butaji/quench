const __readFileAbort = (callback) => {
  const error = new Error("The operation was aborted");
  error.name = "AbortError";
  error.code = "ABORT_ERR";
  callback(error);
};
const __validateReadFileURL = (value) => {
  if (!(value instanceof globalThis.__nodeURL)) return;
  const href = value.href || "";
  if (/%2f/i.test(href)) {
    const error = new TypeError("Invalid file URL path");
    error.code = "ERR_INVALID_FILE_URL_PATH";
    throw error;
  }
  if (value.hostname || /^file:\/\/[^/]+\//i.test(href)) {
    const error = new TypeError("Invalid file URL host");
    error.code = "ERR_INVALID_FILE_URL_HOST";
    throw error;
  }
  if (/%00/i.test(href)) {
    const error = new TypeError("Invalid file URL");
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
};
globalThis.__nodeFs.readFile = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    const error = new TypeError(
      'The "callback" argument must be of type function',
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const encoding = typeof options === "string"
    ? options
    : options && options.encoding;
  if (encoding !== undefined && !NodeBuffer.isEncoding(encoding)) {
    const error = new TypeError(
      `The argument 'encoding' is invalid. Received '${encoding}'`,
    );
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  if (typeof value === "function") {
    const error = new TypeError(
      'The "path" argument must be of type string or an instance of Buffer or URL',
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (value instanceof globalThis.__nodeURL && value.protocol !== "file:") {
    const error = new TypeError("The URL must use the file: protocol");
    error.code = "ERR_INVALID_URL_SCHEME";
    throw error;
  }
  if (value instanceof globalThis.__nodeURL) __validateReadFileURL(value);
  if (value instanceof globalThis.__nodeURL) {
    globalThis.__nodeUrlModule.fileURLToPath(value);
  }
  if (options && options.signal !== undefined) {
    if (!(options.signal instanceof NodeAbortSignal)) {
      const error = new TypeError('The "signal" option must be an AbortSignal');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    __readFileAbort(callback);
    return;
  }
  queueMicrotask(() => {
    try {
      callback(null, globalThis.__nodeFs.readFileSync(value, options));
    } catch (error) {
      callback(error);
    }
  });
};
