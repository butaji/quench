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
  const createURLPattern = () => {
    function URLPattern(options, optionsFlags, baseURL) {
      validateURLPatternInput(
        new.target,
        options,
        optionsFlags,
        baseURL,
        arguments.length
      );
      const patternOptions =
        typeof options === "string" ? new URL(options) : options;
      this.protocol = patternOptions?.protocol || "*";
      this.hostname = patternOptions?.hostname || "*";
      this.pathname = patternOptions?.pathname || "*";
      optionsFlags?.ignoreCase;
      const source = this.pathname;
      this.test = (value, baseURL) =>
        getURLPatternURL(value, baseURL).pathname ===
        source.replace(
          /:[^/]+/g,
          () =>
            getURLPatternURL(value, baseURL).pathname.split("/").slice(-1)[0]
        );
      this.exec = (value, baseURL) => {
        validateURLPatternValue(value, baseURL);
        return getURLPatternResult(source, value, baseURL);
      };
    }
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
        result.URLPattern = createURLPattern();
      }
      return result;
    };
  }
}
