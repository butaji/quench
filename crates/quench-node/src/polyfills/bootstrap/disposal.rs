//! Polyfill: `disposal`

pub const JS: &str = r#"const __quenchChildDisposeRequire = globalThis.require;
const __quenchChildDisposeModule = __quenchChildDisposeRequire("child_process");
const __quenchChildDisposeSpawn = __quenchChildDisposeModule.spawn;
__quenchChildDisposeModule.spawn = (...args) => {
  const child = __quenchChildDisposeSpawn(...args);
  child.destroy = (error) => {
    child.kill(error ? "SIGTERM" : "SIGTERM");
    return child;
  };
  child[Symbol.dispose] = () => {
    child.kill("SIGTERM");
  };
  return child;
};
"#;
