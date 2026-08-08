const __quenchClusterCleanupRequire = globalThis.require;
const __quenchClusterCleanup = __quenchClusterCleanupRequire("cluster");
const __quenchOriginalWorkerKill =
  __quenchClusterCleanup.Worker?.prototype.kill;
if (__quenchOriginalWorkerKill) {
  __quenchClusterCleanup.Worker.prototype.kill = function (...args) {
    const result = __quenchOriginalWorkerKill.apply(this, args);
    if (__quenchClusterCleanup.workers) {
      delete __quenchClusterCleanup.workers[this.id];
    }
    return result;
  };
}
