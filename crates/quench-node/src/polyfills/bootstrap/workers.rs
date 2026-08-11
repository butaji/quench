//! Polyfill: `workers`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchClusterWorkersRequire = globalThis.require;
const __quenchClusterWorkers = __quenchClusterWorkersRequire("cluster");
if (Array.isArray(__quenchClusterWorkers.workers)) {
  const workers = {};
  for (const worker of __quenchClusterWorkers.workers) {
    workers[worker.id] = worker;
  }
  Object.defineProperty(workers, "push", {
    value: (worker) => {
      workers[worker.id] = worker;
      const remove = () => {
        delete workers[worker.id];
      };
      if (typeof worker.prependOnceListener === "function") {
        worker.prependOnceListener("exit", remove);
      } else worker.once("exit", remove);
    },
    enumerable: false,
  });
  Object.defineProperty(workers, Symbol.iterator, {
    value: function* () {
      for (const key of Object.keys(workers)) yield workers[key];
    },
    enumerable: false,
  });
  __quenchClusterWorkers.workers = workers;
}
"#);
