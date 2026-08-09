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
      if (!result.URLPattern) result.URLPattern = createURLPattern();
      result.URL = globalThis.__nodeURL;
      result.URLSearchParams = globalThis.__nodeURLSearchParams;
      installURLCanParse(result.URL);
      installURLToStringDescriptor(result.URL);
    }
    return result;
  };
}
