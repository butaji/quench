//! Polyfill: `process`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchClusterProcessRequire = globalThis.require;
const __quenchClusterProcess = __quenchClusterProcessRequire("cluster");
const __quenchOriginalClusterFork = __quenchClusterProcess.fork;
__quenchClusterProcess.fork = (...args) => {
  const worker = __quenchOriginalClusterFork(...args);
  if (worker.process) {
    worker.process.connected = true;
    worker.process.disconnect = () => worker.disconnect();
    worker.process.send = (...values) => worker.send(...values);
    worker.once("disconnect", () => {
      worker.process.connected = false;
    });
  }
  return worker;
};
"#);
