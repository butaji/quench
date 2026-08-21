//! Polyfill: `module-surface-06-tail`

pub const JS: &str = quench_js_check::checked_js!(r#"if (globalThis.require) {
  const originalRequire = globalThis.require;
  const createURLPattern = globalThis.__quenchURLPatternFactory;
  const installURLCanParse = globalThis.__quenchURLInstallCanParse;
  const installURLToStringDescriptor = globalThis.__quenchURLInstallToString;
  globalThis.require = (name) => {
    let result = originalRequire(name);
    const normalized = String(name).replace(/^node:/, "");
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
if (!globalThis.URLSearchParams) {
  const formEncode = (value) =>
    encodeURIComponent(Array.from(String(value), (part) => {
      const code = part.codePointAt(0);
      return code >= 0xd800 && code <= 0xdfff ? "\ufffd" : part;
    }).join(""));
  globalThis.URLSearchParams = class URLSearchParams {
    constructor() { this._pairs = []; }
    append(name, value) { this._pairs.push([name, value]); }
    toString() {
      return this._pairs
        .map(([name, value]) => `${formEncode(name)}=${formEncode(value)}`)
        .join("&");
    }
  };
}
delete globalThis.__quenchURLPatternFactory;
delete globalThis.__quenchURLInstallCanParse;
delete globalThis.__quenchURLInstallToString;
if (globalThis.require) {
  const path = globalThis.require("path");
  path.toNamespacedPath ||= (value) => value;
  path.matchesGlob ||= (value, pattern) =>
    pattern === "*" ||
    (String(pattern).startsWith("*.") &&
      String(value).endsWith(String(pattern).slice(1)));
}
"#);
