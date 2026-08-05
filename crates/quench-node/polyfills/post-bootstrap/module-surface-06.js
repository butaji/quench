{
  const createURLPattern = () => {
    function URLPattern(options) {
      if (!new.target) {
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
      this.protocol = options?.protocol || "*";
      this.hostname = options?.hostname || "*";
      this.pathname = options?.pathname || "*";
      const source = this.pathname;
      this.test = (value) =>
        new URL(value).pathname ===
        source.replace(
          /:[^/]+/g,
          () => new URL(value).pathname.split("/").slice(-1)[0]
        );
      this.exec = (value) => ({
        pathname: {
          groups: { id: new URL(value).pathname.split("/").slice(-1)[0] }
        }
      });
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
