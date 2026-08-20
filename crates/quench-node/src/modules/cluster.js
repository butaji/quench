// Single-process cluster contract used by the embedded runtime.
module.exports = {
  isPrimary: true, isMaster: true, isWorker: false, worker: null, workers: {},
  SCHED_NONE: 1, SCHED_RR: 2, schedulingPolicy: 2,
  setupPrimary() {}, setupMaster() {}, disconnect(cb) { if (cb) cb(); },
  fork() {
    return {
      isDead() { return false; },
      isConnected() { return false; },
      disconnect(cb) { if (cb) cb(); },
      kill() {},
    };
  }
};
