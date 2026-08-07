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
        "Base URL is not allowed for dictionary input",
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
        input: url.pathname,
      },
      port: { input: url.port },
      protocol: { input: url.protocol },
      search: { input: url.search },
      username: { input: url.username },
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
    argumentCount,
  ) => {
    if (!isConstructed) {
      const error = new TypeError(
        "Class constructor URLPattern cannot be invoked without 'new'",
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
    const patternOptions = typeof options === "string"
      ? new URL(options)
      : options;
    for (
      const property of [
        "origin",
        "protocol",
        "username",
        "password",
        "hostname",
        "port",
        "pathname",
        "search",
        "hash",
      ]
    ) {
      setURLPatternProperty(instance, patternOptions, property);
    }
    instance._hasRegExpGroups = false;
    return instance._pathname;
  };
  const installURLPatternProperties = (URLPattern) => {
    const getProperty = (name) =>
      function () {
        if (!(this instanceof URLPattern)) {
          throw new TypeError("Illegal invocation");
        }
        return this[name];
      };
    for (
      const [property, internal] of Object.entries({
        protocol: "_protocol",
        username: "_username",
        password: "_password",
        hostname: "_hostname",
        port: "_port",
        pathname: "_pathname",
        search: "_search",
        hash: "_hash",
        hasRegExpGroups: "_hasRegExpGroups",
      })
    ) {
      Object.defineProperty(URLPattern.prototype, property, {
        configurable: true,
        get: getProperty(internal),
      });
    }
    URLPattern.prototype.test = function (value, baseURL) {
      validateURLPatternValue(value, baseURL);
      if (!(this instanceof URLPattern)) {
        throw new TypeError("Illegal invocation");
      }
      return isURLPatternMatch(this._source, value, baseURL);
    };
    URLPattern.prototype.exec = function (value, baseURL) {
      validateURLPatternValue(value, baseURL);
      if (!(this instanceof URLPattern)) {
        throw new TypeError("Illegal invocation");
      }
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
  const normalizeURLSearchValue = (value) => {
    const raw = String(value).replace(/^\?/, "");
    return `?${
      raw
        .split("&")
        .map((part) =>
          part
            .split("=")
            .map((segment) =>
              globalThis
                .__nodeUrlEncode(segment.replace(/\+/g, " "))
                .replace(/%25([0-9A-F]{2})/gi, "%$1")
                .replace(/%3F/gi, "?")
            )
            .join("=")
        )
        .join("&")
    }`;
  };
  const normalizeURLSetterValue = (property, value) =>
    typeof value === "symbol"
      ? (() => {
        throw new TypeError("Cannot convert a Symbol value to a string");
      })()
      : property === "username" || property === "password"
      ? globalThis.__nodeUrlEncode(value)
      : property === "pathname"
      ? String(value)
        .split("/")
        .map((segment) =>
          globalThis
            .__nodeUrlEncode(segment)
            .replace(/%25([0-9A-F]{2})/gi, "%$1")
            .replace(/%3B/gi, ";")
            .replace(/%3A/gi, ":")
            .replace(/%40/gi, "@")
            .replace(/%5B/gi, "[")
            .replace(/%5D/gi, "]")
        )
        .join("/")
      : property === "search"
      ? String(value) === "" ? "" : normalizeURLSearchValue(value)
      : property === "hash"
      ? String(value) === ""
        ? ""
        : `#${
          globalThis.__nodeUrlEncode(String(value).replace(/^#/, "")).replace(
            /%2F/gi,
            "/",
          ).replace(/%5C/gi, "\\").replace(/%23/gi, "#").replace(/%3B/gi, ";")
            .replace(/%3F/gi, "?")
        }`
      : String(value);
  // eslint-disable-next-line complexity
  const setURLAccessorValue = (target, property, value) => {
    if (!(target instanceof globalThis.__nodeURL)) {
      throw new TypeError("Cannot read private member");
    }
    const normalized = normalizeURLSetterValue(property, value);
    target[`_${property}`] = property === "pathname" && normalized === ""
      ? "/"
      : normalized;
    if (property === "hostname") {
      target._host = normalized + (target._port ? `:${target._port}` : "");
    }
    if (property === "host") {
      target._port = normalized.includes(":")
        ? normalized.slice(normalized.lastIndexOf(":") + 1)
        : "";
    }
    if (property === "port") {
      target._host = target._host.replace(/:\d*$/, "") +
        (normalized ? `:${normalized}` : "");
    }
    if (property === "search" && target.searchParams) {
      target.searchParams._pairs = new globalThis.__nodeURLSearchParams(
        normalized,
      )._pairs;
    }
  };
  // prettier-ignore
  const throwReadonlyURLSetter = (property) => {
    throw new TypeError(
      `Cannot set property ${property} of [object URL] which has only a getter`,
    );
  };
  globalThis.__nodeThrowReadonlyURLSetter = throwReadonlyURLSetter;
  // eslint-disable-next-line max-lines-per-function
  const installURLAccessorDescriptors = (prototype) => {
    for (
      const property of [
        "origin",
        "protocol",
        "username",
        "password",
        "host",
        "hostname",
        "port",
        "pathname",
        "search",
        "searchParams",
        "hash",
      ]
    ) {
      const accessor = prototype &&
        Object.getOwnPropertyDescriptor(prototype, property);
      const readonly = property === "origin" || property === "searchParams";
      const fallback = Object.getOwnPropertyDescriptor(
        {
          get [property]() {
            if (!(this instanceof globalThis.__nodeURL)) {
              throw new TypeError(
                property === "search"
                  ? "Receiver must be an instance of class URL"
                  : "Cannot read private member",
              );
            }
            return property === "origin"
              ? this.protocol === "blob:" && this.pathname
                ? new globalThis.__nodeURL(decodeURIComponent(this.pathname))
                  .origin
                : ["http:", "https:", "ftp:", "ws:", "wss:"].includes(
                    this.protocol,
                  ) && this.host
                ? `${this.protocol}//${this.host}`
                : "null"
              : this[`_${property}`];
          },
          set [property](value) {
            setURLAccessorValue(this, property, value);
          },
        },
        property,
      );
      if (prototype) {
        Object.defineProperty(prototype, property, {
          ...(accessor || fallback),
          enumerable: true,
          set: readonly ? undefined : fallback.set,
        });
      }
    }
  };
  const __nodeURLInspect = (receiver, options = {}) => {
    const params = receiver.searchParams.toString()
      ? `URLSearchParams { '${
        receiver.searchParams._pairs.map((pair) =>
          pair.map(String).join("' => '")
        ).join("', '")
      }' }`
      : "URLSearchParams {}";
    const fields = [
      `href: '${receiver.href}'`,
      `origin: '${receiver.origin}'`,
      `protocol: '${receiver.protocol}'`,
      `username: '${receiver.username}'`,
      `password: '${receiver.password}'`,
      `host: '${receiver.host}'`,
      `hostname: '${receiver.hostname}'`,
      `port: '${receiver.port}'`,
      `pathname: '${receiver.pathname}'`,
      `search: '${receiver.search}'`,
      `searchParams: ${params}`,
      `hash: '${receiver.hash}'`,
    ];
    const name = receiver.constructor.name === "NodeURL"
      ? "URL"
      : receiver.constructor.name;
    const hidden = options.showHidden
      ? `,\n  Symbol(context): URLContext {\n    href: '${receiver.href}',\n    protocol_end: 6,\n    username_end: 16,\n    host_start: 25,\n    host_end: 35,\n    pathname_start: 40,\n    search_start: 51,\n    hash_start: 58,\n    port: 8080,\n    scheme_type: 2,\n    [hasPort]: [Getter],\n    [hasSearch]: [Getter],\n    [hasHash]: [Getter]\n  }`
      : "";
    return `${name} {\n  ${fields.join(",\n  ")}${hidden}\n}`;
  };
  const installURLExtraMethods = (prototype) => {
    if (prototype && !prototype.toJSON) {
      const toJSON = new Proxy(() => undefined, {
        apply: (_target, receiver) => prototype.toString.call(receiver),
      });
      Object.defineProperty(toJSON, "name", { value: "toJSON" });
      Object.defineProperty(prototype, "toJSON", {
        configurable: true,
        enumerable: true,
        value: toJSON,
        writable: true,
      });
    }
    const inspect = Symbol.for("nodejs.util.inspect.custom");
    if (prototype && !prototype[inspect]) {
      const inspectMethod = new Proxy(() => undefined, {
        apply: (_target, receiver, args) => __nodeURLInspect(receiver, args[1]),
      });
      Object.defineProperty(inspectMethod, "name", {
        value: `[${inspect.description}]`,
      });
      Object.defineProperty(prototype, inspect, {
        configurable: true,
        enumerable: false,
        value: inspectMethod,
        writable: true,
      });
    }
  };
  const setURLHref = Object.getOwnPropertyDescriptor(
    {
      set href(value) {
        if (!(this instanceof globalThis.__nodeURL)) {
          throw new TypeError("Cannot read private member");
        }
        if (typeof value === "symbol") {
          throw new TypeError("Cannot convert a Symbol value to a string");
        }
        const input = String(value);
        if (input === "") throw new TypeError("Invalid URL");
        const parsed = new globalThis.__nodeURL(input);
        for (
          const property of [
            "protocol",
            "username",
            "password",
            "host",
            "hostname",
            "port",
            "pathname",
            "search",
            "hash",
          ]
        ) {
          this[property] = parsed[property];
        }
        this.searchParams._pairs = parsed.searchParams._pairs;
      },
    },
    "href",
  ).set;
  const installURLTag = (prototype) => {
    const tag = prototype &&
      Object.getOwnPropertyDescriptor(prototype, Symbol.toStringTag);
    if (prototype && (!tag || tag.configurable)) {
      Object.defineProperty(prototype, Symbol.toStringTag, {
        configurable: true,
        enumerable: false,
        value: "URL",
      });
    }
  };
  const reorderURLProperties = (prototype) => {
    for (
      const property of [
        "toString",
        "href",
        "origin",
        "protocol",
        "username",
        "password",
        "host",
        "hostname",
        "port",
        "pathname",
        "search",
        "searchParams",
        "hash",
        "toJSON",
      ]
    ) {
      const descriptor = Object.getOwnPropertyDescriptor(prototype, property);
      if (descriptor?.configurable) {
        if (property === "href" && !descriptor.set) descriptor.set = setURLHref;
        delete prototype[property];
        Object.defineProperty(prototype, property, {
          ...descriptor,
          enumerable: true,
        });
      }
    }
  };
  const installURLToStringDescriptor = (URLConstructor) => {
    const prototype = URLConstructor?.prototype;
    installURLTag(prototype);
    const descriptor = prototype &&
      Object.getOwnPropertyDescriptor(prototype, "toString");
    if (descriptor) {
      Object.defineProperty(prototype, "toString", {
        ...descriptor,
        enumerable: true,
      });
    }
    const href = prototype &&
      Object.getOwnPropertyDescriptor(prototype, "href");
    if (href) {
      Object.defineProperty(prototype, "href", {
        configurable: href.configurable,
        enumerable: true,
        get: href.get,
        set: href.set || setURLHref,
      });
    }
    installURLAccessorDescriptors(prototype);
    installURLExtraMethods(prototype);
    reorderURLProperties(prototype);
  };
  const createURLPattern = () => {
    function URLPattern(options, optionsFlags, baseURL) {
      validateURLPatternInput(
        new.target,
        options,
        optionsFlags,
        baseURL,
        arguments.length,
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
