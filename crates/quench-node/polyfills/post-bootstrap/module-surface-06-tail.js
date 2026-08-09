if (globalThis.require) {
  const path = globalThis.require("path");
  path.toNamespacedPath ||= (value) => value;
  path.matchesGlob ||= (value, pattern) =>
    pattern === "*" ||
    (String(pattern).startsWith("*.") &&
      String(value).endsWith(String(pattern).slice(1)));
}
