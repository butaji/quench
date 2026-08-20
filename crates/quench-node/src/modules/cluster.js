// Single-process cluster contract used by the embedded runtime.
module.exports = {
  isPrimary: true, isMaster: true, isWorker: false, worker: null, workers: {},
  SCHED_NONE: 1, SCHED_RR: 2, schedulingPolicy: 2,
  setupPrimary() {}, setupMaster() {}, disconnect(cb) { if (cb) cb(); },
  fork() { throw new Error('cluster.fork is unavailable in the embedded runtime'); }
};
