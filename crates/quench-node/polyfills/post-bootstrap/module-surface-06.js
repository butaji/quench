{
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
        result.URLPattern = function URLPattern(options) {
          const source = options?.pathname || "*";
          this.test = (value) =>
            new URL(value).pathname ===
            source.replace(/:[^/]+/g, (part) =>
              part ? new URL(value).pathname.split("/").slice(-1)[0] : part
            );
          this.exec = (value) => ({
            pathname: {
              groups: { id: new URL(value).pathname.split("/").slice(-1)[0] }
            }
          });
        };
      }
      return result;
    };
  }
}
