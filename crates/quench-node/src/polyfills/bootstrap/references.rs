//! Polyfill: `references`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchChildRefRequire = globalThis.require;
const __quenchChildRefModule = __quenchChildRefRequire("child_process");
const __quenchRefSpawn = __quenchChildRefModule.spawn;
__quenchChildRefModule.spawn = (...args) => {
  const child = __quenchRefSpawn(...args);
  if (typeof child.ref !== "function") child.ref = () => undefined;
  return child;
};
"#);
