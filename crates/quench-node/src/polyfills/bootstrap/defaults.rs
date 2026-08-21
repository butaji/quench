//! Polyfill: `defaults`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchClusterDefaultsRequire = globalThis.require;
const __quenchClusterDefaults = __quenchClusterDefaultsRequire("cluster");
const __quenchSetupWithCumulativeSettings =
  __quenchClusterDefaults.setupPrimary;
const __quenchClusterInitialSettings = (current) =>
  Object.keys(current).length === 0
    ? {
      args: globalThis.process?.argv?.slice(2) || [],
      exec: globalThis.process?.argv?.[1],
      execArgv: globalThis.process?.execArgv || [],
      silent: false,
    }
    : {};
__quenchClusterDefaults.setupPrimary = (options = {}) => {
  const current = __quenchClusterDefaults.settings || {};
  return __quenchSetupWithCumulativeSettings({
    ...__quenchClusterInitialSettings(current),
    ...current,
    ...options,
  });
};
"#);
