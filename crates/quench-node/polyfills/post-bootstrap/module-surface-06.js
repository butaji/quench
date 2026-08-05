{
  const validateURLPatternValue = (value, baseURL) => {
    if (value != null && typeof value !== "string") {
      const error = new TypeError("Invalid URLPattern input");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (baseURL != null && typeof baseURL !== "string") {
      const error = new TypeError("Invalid URLPattern base URL");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (value === null && baseURL === null) {
      const error = new TypeError(
        "Base URL is not allowed for dictionary input"
      );
      error.code = "ERR_OPERATION_FAILED";
      throw error;
    }
  };
  const getURLPatternURL = (value, baseURL) => {
    validateURLPatternValue(value, baseURL);
    return new URL(value ?? "", baseURL ?? "https://example.com");
  };
  const getURLPatternGroups = (source, pathname) => {
    const names = [...source.matchAll(/:([^/]+)/g)].map((match) => match[1]);
    const value = pathname.split("/").slice(-1)[0];
    return Object.fromEntries(names.map((name) => [name, value]));
  };
  const getURLPatternResult = (source, value, baseURL) => {
    const url = getURLPatternURL(value, baseURL);
    return {
      hash: { input: url.hash },
      hostname: { input: url.hostname },
      inputs: [value],
      password: { input: url.password },
      pathname: {
        groups: getURLPatternGroups(source, url.pathname),
        input: url.pathname
      },
      port: { input: url.port },
      protocol: { input: url.protocol },
      search: { input: url.search },
      username: { input: url.username }
    };
  };
  const isURLPatternMatch = (source, value, baseURL) => {
    if (source === "*") return baseURL !== null;
    const pathname = getURLPatternURL(value, baseURL).pathname;
    return (
      pathname ===
      source.replace(/:[^/]+/g, () => pathname.split("/").slice(-1)[0])
    );
  };
  const validateURLPatternBase = (baseURL) => {
    if (baseURL != null && typeof baseURL !== "string") {
      const error = new TypeError("Invalid URLPattern base URL");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
  };
  const validateURLPatternInput = (
    isConstructed,
    options,
    optionsFlags,
    baseURL,
    argumentCount
  ) => {
    if (!isConstructed) {
      const error = new TypeError(
        "Class constructor URLPattern cannot be invoked without 'new'"
      );
      error.code = "ERR_CONSTRUCT_CALL_REQUIRED";
      throw error;
    }
    if (
      options != null &&
      typeof options !== "object" &&
      typeof options !== "string"
    ) {
      const error = new TypeError("Invalid URLPattern input");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (typeof optionsFlags === "number") {
      const error = new TypeError("Invalid URLPattern options");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    validateURLPatternBase(baseURL);
    if (argumentCount >= 3 && baseURL == null && optionsFlags == null) {
      const error = new TypeError("Invalid URLPattern base URL");
      error.code = "ERR_INVALID_URL_PATTERN";
      throw error;
    }
  };
  const setURLPatternProperty = (instance, options, property) => {
    instance[`_${property}`] = options?.[property] || "*";
  };
  const initializeURLPattern = (instance, options) => {
    const patternOptions =
      typeof options === "string" ? new URL(options) : options;
    for (const property of [
      "protocol",
      "username",
      "password",
      "hostname",
      "port",
      "pathname",
      "search",
      "hash"
    ]) {
      setURLPatternProperty(instance, patternOptions, property);
    }
    instance._hasRegExpGroups = false;
    return instance._pathname;
  };
  const installURLPatternProperties = (URLPattern) => {
    const getProperty = (name) =>
      function () {
        if (!(this instanceof URLPattern))
          throw new TypeError("Illegal invocation");
        return this[name];
      };
    for (const [property, internal] of Object.entries({
      protocol: "_protocol",
      username: "_username",
      password: "_password",
      hostname: "_hostname",
      port: "_port",
      pathname: "_pathname",
      search: "_search",
      hash: "_hash",
      hasRegExpGroups: "_hasRegExpGroups"
    })) {
      Object.defineProperty(URLPattern.prototype, property, {
        configurable: true,
        get: getProperty(internal)
      });
    }
    URLPattern.prototype.test = function (value, baseURL) {
      if (!(this instanceof URLPattern))
        throw new TypeError("Illegal invocation");
      validateURLPatternValue(value, baseURL);
      return isURLPatternMatch(this._source, value, baseURL);
    };
    URLPattern.prototype.exec = function (value, baseURL) {
      if (!(this instanceof URLPattern))
        throw new TypeError("Illegal invocation");
      validateURLPatternValue(value, baseURL);
      if (baseURL === null && typeof value === "string") return null;
      return getURLPatternResult(this._source, value, baseURL);
    };
  };
  const installURLCanParse = (urlConstructor) => {
    const original = urlConstructor.canParse;
    if (typeof original !== "function") return;
    urlConstructor.canParse = function (value, base) {
      if (!arguments.length) {
        const error = new TypeError("The url argument must be specified");
        error.code = "ERR_MISSING_ARGS";
        throw error;
      }
      return original.call(this, value, base);
    };
  };
  const installURLToStringDescriptor = (URLConstructor) => {
    const prototype = URLConstructor?.prototype;
    const descriptor =
      prototype && Object.getOwnPropertyDescriptor(prototype, "toString");
    if (descriptor)
      Object.defineProperty(prototype, "toString", {
        ...descriptor,
        enumerable: true
      });
  };
  const createURLPattern = () => {
    function URLPattern(options, optionsFlags, baseURL) {
      validateURLPatternInput(
        new.target,
        options,
        optionsFlags,
        baseURL,
        arguments.length
      );
      const source = initializeURLPattern(this, options);
      this._source = source;
      optionsFlags?.ignoreCase;
    }
    installURLPatternProperties(URLPattern);
    return URLPattern;
  };

  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      let result = originalRequire(name);
      const normalized = String(name).replace(/^node:/, "");
      if (normalized === "path") {
        result.toNamespacedPath ||= (value) => value;
        result.matchesGlob ||= (value, pattern) =>
          pattern === "*" ||
          (String(pattern).startsWith("*.") &&
            String(value).endsWith(String(pattern).slice(1)));
      }
      if (normalized === "url" && !result.URLPattern) {
        result = Object.assign({}, result);
        installURLCanParse(result.URL);
        installURLToStringDescriptor(result.URL);
        result.URLPattern = createURLPattern();
      }
      return result;
    };
  }
}
