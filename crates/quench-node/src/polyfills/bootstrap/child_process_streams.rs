//! Polyfill: `child-process-streams`

pub const JS: &str = r#"const __quenchChildStreamRequire = globalThis.require;
const __quenchChildStreamModule = __quenchChildStreamRequire("child_process");
const __quenchOriginalChildStreamSpawn = __quenchChildStreamModule.spawn;
const __quenchEnsureStream = (stream) => {
  if (typeof stream.on !== "function") stream.on = () => stream;
  if (typeof stream.once !== "function") stream.once = stream.on;
  if (typeof stream.setEncoding !== "function") {
    stream.setEncoding = () => stream;
  }
  return stream;
};
__quenchChildStreamModule.spawn = (...args) => {
  const child = __quenchOriginalChildStreamSpawn(...args);
  __quenchEnsureStream(child.stdin);
  __quenchEnsureStream(child.stdout);
  __quenchEnsureStream(child.stderr);
  return child;
};
"#;
