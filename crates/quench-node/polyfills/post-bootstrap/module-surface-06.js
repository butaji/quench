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
  const installURLAccessorDescriptors = (prototype) => {
    for (const property of [
      "protocol",
      "username",
      "password",
      "host",
      "hostname",
      "port",
      "pathname",
      "search",
      "hash",
      "origin",
      "searchParams"
    ]) {
      const accessor =
        prototype && Object.getOwnPropertyDescriptor(prototype, property);
      const readonly = property === "origin" || property === "searchParams";
      const fallback = Object.getOwnPropertyDescriptor(
        {
          get [property]() {
            return this[`_${property}`];
          },
          set [property](value) {
            this[`_${property}`] =
              property === "username" || property === "password"
                ? globalThis.__nodeUrlEncode(value)
                : value;
          }
        },
        property
      );
      if (prototype)
        Object.defineProperty(prototype, property, {
          ...(accessor || fallback),
          enumerable: true,
          set: accessor?.set || (readonly ? undefined : fallback.set)
        });
    }
  };
  const installURLExtraMethods = (prototype) => {
    if (prototype && !prototype.toJSON) {
      const toJSON = new Proxy(() => undefined, {
        apply: (_target, receiver) => prototype.toString.call(receiver)
      });
      Object.defineProperty(toJSON, "name", { value: "toJSON" });
      Object.defineProperty(prototype, "toJSON", {
        configurable: true,
        enumerable: true,
        value: toJSON,
        writable: true
      });
    }
    const inspect = Symbol.for("nodejs.util.inspect.custom");
    if (prototype && !prototype[inspect]) {
      const inspectMethod = new Proxy(() => undefined, {
        apply: (_target, receiver) => prototype.toString.call(receiver)
      });
      Object.defineProperty(inspectMethod, "name", {
        value: `[${inspect.description}]`
      });
      Object.defineProperty(prototype, inspect, {
        configurable: true,
        enumerable: false,
        value: inspectMethod,
        writable: true
      });
    }
  };
  const setURLHref = Object.getOwnPropertyDescriptor(
    {
      set href(value) {
        const parsed = new globalThis.__nodeURL(String(value));
        for (const property of [
          "protocol",
          "username",
          "password",
          "host",
          "hostname",
          "port",
          "pathname",
          "search",
          "hash",
          "origin"
        ])
          this[property] = parsed[property];
        this.searchParams._pairs = parsed.searchParams._pairs;
      }
    },
    "href"
  ).set;
  const installURLToStringDescriptor = (URLConstructor) => {
    const prototype = URLConstructor?.prototype;
    const descriptor =
      prototype && Object.getOwnPropertyDescriptor(prototype, "toString");
    if (descriptor)
      Object.defineProperty(prototype, "toString", {
        ...descriptor,
        enumerable: true
      });
    const href =
      prototype && Object.getOwnPropertyDescriptor(prototype, "href");
    if (href) {
      Object.defineProperty(prototype, "href", {
        configurable: href.configurable,
        enumerable: true,
        get: href.get,
        set: href.set || setURLHref
      });
    }
    installURLAccessorDescriptors(prototype);
    installURLExtraMethods(prototype);
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

  installURLToStringDescriptor(globalThis.__nodeURL);
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
      if (normalized === "url") {
        result = Object.assign({}, result);
        if (!result.URLPattern) {
          result.URLPattern = createURLPattern();
        }
        result.URL = globalThis.__nodeURL;
        result.URLSearchParams = globalThis.__nodeURLSearchParams;
        installURLCanParse(result.URL);
        installURLToStringDescriptor(result.URL);
      }
      return result;
    };
  }
}
