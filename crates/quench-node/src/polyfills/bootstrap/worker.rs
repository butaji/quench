//! Polyfill: `worker`

pub const JS: &str = r#"const __quenchClusterWorkerRequire = globalThis.require;
const __quenchClusterWorkerModule = __quenchClusterWorkerRequire("cluster");
const __quenchWorkerPrototype = __quenchClusterWorkerModule.Worker?.prototype;
if (
  __quenchWorkerPrototype &&
  typeof __quenchWorkerPrototype.isDead !== "function"
) {
  __quenchWorkerPrototype.isDead = function () {
    return (
      this.state === "dead" ||
      this.process?.exitCode != null ||
      this.process?.signalCode != null
    );
  };
}
if (
  __quenchWorkerPrototype &&
  typeof __quenchWorkerPrototype.destroy !== "function"
) {
  __quenchWorkerPrototype.destroy = function (signal) {
    return this.kill(signal);
  };
}
"#;
