//! Polyfill: `setup`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchClusterSetupRequire = globalThis.require;
const __quenchClusterSetup = __quenchClusterSetupRequire("cluster");
const __quenchOriginalSetupPrimary = __quenchClusterSetup.setupPrimary;
__quenchClusterSetup.setupPrimary = (options = {}) =>
  __quenchOriginalSetupPrimary({
    ...__quenchClusterSetup.settings,
    ...options,
  });
"#);
