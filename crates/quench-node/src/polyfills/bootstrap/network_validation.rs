//! Polyfill: `network-validation`

pub const JS: &str = r#"const __quenchValidatePort = (value) => {
  if (typeof value !== "number" && typeof value !== "string") {
    throw Object.assign(new TypeError('The "options.port" property must be a number or string'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (typeof value === "string" && value.trim() === "") {
    throw Object.assign(new RangeError("Port should be >= 0 and < 65536."), { code: "ERR_SOCKET_BAD_PORT" });
  }
  const port = Number(value);
  if (
    !Number.isFinite(port) ||
    !Number.isInteger(port) ||
    port < 0 ||
    port > 65535
  ) {
    throw Object.assign(new RangeError(`Port should be >= 0 and < 65536. Received ${value}.`), { code: "ERR_SOCKET_BAD_PORT" });
  }
  return port;
};
globalThis.__quenchValidateConnectionOptions = (options) => {
  if (options && typeof options === "object" && !Array.isArray(options)) {
    if (options.path !== undefined) {
      if (typeof options.path !== "string") {
        throw Object.assign(new TypeError('The "path" argument must be a string'), { code: "ERR_INVALID_ARG_TYPE" });
      }
      return;
    }
    if (options.hints !== undefined) {
      throw Object.assign(new TypeError(`The argument 'hints' is invalid. Received ${options.hints}`), { code: "ERR_INVALID_ARG_VALUE" });
    }
    return __quenchValidatePort(options.port);
  }
  return __quenchValidatePort(options);
};
"#;
