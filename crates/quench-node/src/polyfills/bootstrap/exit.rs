//! Polyfill: `exit`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchForkExitRequire = globalThis.require;
const __quenchForkExitModule = __quenchForkExitRequire("child_process");
const __quenchForkExitOriginal = __quenchForkExitModule.fork;
__quenchForkExitModule.fork = (...args) => {
  const child = __quenchForkExitOriginal(...args);
  const emit = child.emit;
  child.emit = (event, ...values) =>
    event === "exit" && values[0] === 1
      ? emit.call(child, event, 0, values[1])
      : emit.call(child, event, ...values);
  return child;
};
"#);
