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
    throw Object.assign(new TypeError("Invalid file URL path"), { code: "ERR_INVALID_FILE_URL_PATH" });
  }
  if (value.hostname || /^file:\/\/[^/]+\//i.test(href)) {
    throw Object.assign(new TypeError("Invalid file URL host"), { code: "ERR_INVALID_FILE_URL_HOST" });
  }
  if (/%00/i.test(href)) {
    throw Object.assign(new TypeError("Invalid file URL"), { code: "ERR_INVALID_ARG_VALUE" });
  }
};
const __readFileValidatePath = (value) => {
  if (typeof value === "function") {
    throw Object.assign(new TypeError('The "path" argument must be a path'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (value instanceof globalThis.__nodeURL && value.protocol !== "file:") {
    throw Object.assign(new TypeError("The URL must use the file: protocol"), { code: "ERR_INVALID_URL_SCHEME" });
  }
  if (value instanceof globalThis.__nodeURL) {
    __validateReadFileURL(value);
    globalThis.__nodeUrlModule.fileURLToPath(value);
  }
};
const __readFileValidateOptions = (options) => {
  const encoding = typeof options === "string" ? options : options?.encoding;
  if (encoding !== undefined && !NodeBuffer.isEncoding(encoding)) {
    throw Object.assign(new TypeError(`The argument 'encoding' is invalid. Received '${encoding}'`), { code: "ERR_INVALID_ARG_VALUE" });
  }
};
globalThis.__nodeFs.readFile = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    throw Object.assign(new TypeError('The "callback" argument must be of type function'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  __readFileValidateOptions(options);
  __readFileValidatePath(value);
  if (options && options.signal !== undefined) {
    if (!(options.signal instanceof NodeAbortSignal)) {
      throw Object.assign(new TypeError('The "signal" option must be an AbortSignal'), { code: "ERR_INVALID_ARG_TYPE" });
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
