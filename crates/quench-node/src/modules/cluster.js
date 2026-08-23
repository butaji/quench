// Single-process cluster contract. Child processes are unavailable.
var workers = {};
var nextId = 1;
function workerHandle(id) {
  var listeners = {};
  var connected = true;
  var dead = false;
  var worker = {
    id: id,
    process: null,
    exitedAfterDisconnect: false,
    isDead: function () { return dead; },
    isConnected: function () { return connected && !dead; },
    on: function (name, fn) {
      if (!listeners[name]) listeners[name] = [];
      listeners[name].push(fn);
      return worker;
    },
    emit: function (name) {
      var args = Array.prototype.slice.call(arguments, 1);
      var list = listeners[name] || [];
      for (var i = 0; i < list.length; i++) list[i].apply(worker, args);
      return list.length > 0;
    },
    disconnect: function (cb) {
      if (!connected && dead) {
        if (cb) cb();
        return worker;
      }
      worker.exitedAfterDisconnect = true;
      connected = false;
      worker.emit('disconnect');
      if (cb) cb();
      return worker;
    },
    kill: function (signal, cb) {
      if (typeof signal === 'function') { cb = signal; signal = undefined; }
      connected = false;
      dead = true;
      worker.emit('exit', 0, signal || 'SIGTERM');
      delete workers[id];
      if (cb) cb();
      return worker;
    }
  };
  return worker;
}
var api = {
  isPrimary: true,
  isMaster: true,
  isWorker: false,
  worker: null,
  workers: workers,
  SCHED_NONE: 1,
  SCHED_RR: 2,
  schedulingPolicy: 2,
  on: function () { return api; },
  emit: function () { return false; },
  setupPrimary: function () {},
  setupMaster: function () {},
  disconnect: function (cb) {
    var keys = Object.keys(workers);
    for (var i = 0; i < keys.length; i++) workers[keys[i]].disconnect();
    if (cb) cb();
  },
  fork: function (env) {
    var worker = workerHandle(nextId++);
    worker.process = { pid: undefined, env: env || {} };
    workers[worker.id] = worker;
    return worker;
  }
};
module.exports = api;