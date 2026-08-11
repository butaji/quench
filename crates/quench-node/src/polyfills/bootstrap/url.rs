//! Polyfill: `url`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithUrlStatics = globalThis.require;
const __quenchUrlParse = (input, base) => {
  if (base === undefined && !/^[A-Za-z][A-Za-z0-9+.-]*:/.test(String(input))) {
    return null;
  }
  try {
    return new URL(input, base);
  } catch {
    return null;
  }
};
if (typeof URL.canParse !== "function") {
  URL.canParse = (input, base) => __quenchUrlParse(input, base) !== null;
}
if (typeof URL.parse !== "function") URL.parse = __quenchUrlParse;
globalThis.require = (specifier) =>
  __quenchOriginalRequireWithUrlStatics(specifier);
"#);
