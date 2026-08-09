const __quenchValidatePort = (value) => {
  if (typeof value !== "number" && typeof value !== "string") {
    const error = new TypeError(
      'The "options.port" property must be a number or string',
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof value === "string" && value.trim() === "") {
    const error = new RangeError("Port should be >= 0 and < 65536.");
    error.code = "ERR_SOCKET_BAD_PORT";
    throw error;
  }
  const port = Number(value);
  if (
    !Number.isFinite(port) ||
    !Number.isInteger(port) ||
    port < 0 ||
    port > 65535
  ) {
    const error = new RangeError(
      `Port should be >= 0 and < 65536. Received ${value}.`,
    );
    error.code = "ERR_SOCKET_BAD_PORT";
    throw error;
  }
  return port;
};
globalThis.__quenchValidateConnectionOptions = (options) => {
  if (options && typeof options === "object" && !Array.isArray(options)) {
    if (options.path !== undefined) {
      if (typeof options.path !== "string") {
        const error = new TypeError("The \"path\" argument must be a string");
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      return;
    }
    if (options.hints !== undefined) {
      const error = new TypeError(
        `The argument 'hints' is invalid. Received ${options.hints}`,
      );
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    return __quenchValidatePort(options.port);
  }
  return __quenchValidatePort(options);
};
