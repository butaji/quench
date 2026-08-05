{
  const getURLPatternURL = (value) => new URL(value);
  const getURLPatternGroups = (source, pathname) => {
    const names = [...source.matchAll(/:([^/]+)/g)].map((match) => match[1]);
    const value = pathname.split("/").slice(-1)[0];
    return Object.fromEntries(names.map((name) => [name, value]));
  };
  const getURLPatternResult = (source, value) => {
    const url = getURLPatternURL(value);
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
  const validateURLPatternInput = (isConstructed, options) => {
    if (!isConstructed) {
      const error = new TypeError(
        "Class constructor URLPattern cannot be invoked without 'new'"
      );
      error.code = "ERR_CONSTRUCT_CALL_REQUIRED";
      throw error;
    }
    if (options != null && typeof options !== "object") {
      const error = new TypeError("Invalid URLPattern input");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
  };
  const createURLPattern = () => {
    function URLPattern(options, optionsFlags) {
      validateURLPatternInput(new.target, options);
      this.protocol = options?.protocol || "*";
      this.hostname = options?.hostname || "*";
      this.pathname = options?.pathname || "*";
      optionsFlags?.ignoreCase;
      const source = this.pathname;
      this.test = (value) =>
        getURLPatternURL(value).pathname ===
        source.replace(
          /:[^/]+/g,
          () => getURLPatternURL(value).pathname.split("/").slice(-1)[0]
        );
      this.exec = (value) => getURLPatternResult(source, value);
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
