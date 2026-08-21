//! Polyfill: `module-surface-11-tail`

pub const JS: &str = quench_js_check::checked_js!(r#"for (
  const symbol of [
    Symbol.iterator,
    Symbol.for("nodejs.util.inspect.custom"),
  ]
) {
  const descriptor = Object.getOwnPropertyDescriptor(
    globalThis.__nodeURLSearchParams.prototype,
    symbol,
  );
  Object.defineProperty(globalThis.__nodeURLSearchParams.prototype, symbol, {
    ...descriptor,
    enumerable: false,
  });
}
"#);
