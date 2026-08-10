//! Polyfill: `fork`

pub const JS: &str = r#"const __quenchForkChildRequire = globalThis.require;
const __quenchForkChildModule = __quenchForkChildRequire("child_process");
if (__quenchForkChildModule._forkChild === undefined) {
  __quenchForkChildModule._forkChild = (fd, options) => undefined;
}
"#;
