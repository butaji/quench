//! Polyfill: `ref`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchProcessRef = globalThis.process;
const __quenchRefSymbol = Symbol.for("nodejs.ref");
const __quenchUnrefSymbol = Symbol.for("nodejs.unref");
__quenchProcessRef.ref = (value) => {
  if (value?.[__quenchRefSymbol]) value[__quenchRefSymbol]();
  else value?.ref?.();
};
__quenchProcessRef.unref = (value) => {
  if (value?.[__quenchUnrefSymbol]) value[__quenchUnrefSymbol]();
  else value?.unref?.();
};
"#);
