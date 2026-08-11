//! Polyfill: `fork`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchForkChildRequire = globalThis.require;
const __quenchForkChildModule = __quenchForkChildRequire("child_process");
if (__quenchForkChildModule._forkChild === undefined) {
  __quenchForkChildModule._forkChild = (fd, options) => undefined;
}
"#);
