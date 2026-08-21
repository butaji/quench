// Worker adapter: execute worker files in a separate quench-node process.
// The synchronous launch is intentional: it gives the small embedding a real
// isolated VM while preserving deterministic event delivery to the parent.
var proc = process;
var workerEnv = proc.env && proc.env.QUENCH_WORKER;
var workerData = workerEnv ? JSON.parse(proc.env.QUENCH_WORKER_DATA || 'null') : null;
function emitter(target) {
  var listeners = {};
  var pending = target._queueEvents ? [] : null;
  target.on = function (name, fn) {
    (listeners[name] || (listeners[name] = [])).push(fn);
    if (pending) {
      var keep = [];
      for (var p = 0; p < pending.length; p++) {
        if (pending[p][0] === name) fn.apply(target, pending[p][1]);
        else keep.push(pending[p]);
      }
      pending = keep;
    }
    return target;
  };
  target.once = function (name, fn) {
    function once() { target.removeListener(name, once); fn.apply(target, arguments); }
    return target.on(name, once);
  };
  target.removeListener = function (name, fn) {
    var a = listeners[name] || [], i = a.indexOf(fn); if (i >= 0) a.splice(i, 1); return target;
  };
  target.emit = function (name) {
    var a = (listeners[name] || []).slice(), args = Array.prototype.slice.call(arguments, 1);
    if (pending && a.length === 0) pending.push([name, args]);
    for (var i = 0; i < a.length; i++) a[i].apply(target, args);
    return a.length > 0;
  };
  return target;
}
var parentPort = workerEnv ? emitter({
  postMessage: function (value) { console.log('__QUENCH_WORKER_MESSAGE__' + JSON.stringify(value)); },
  close: function () {}
}) : null;
var api = {
  isMainThread: !workerEnv, threadId: workerEnv ? 1 : 0, workerData: workerData,
  parentPort: parentPort, MessageChannel: function MessageChannel() {},
  MessagePort: function MessagePort() {},
  Worker: function Worker(filename, options) {
    var self = emitter({ threadId: 1, exited: false, _queueEvents: true });
    options = options || {};
    var cp = require('child_process');
    var env = {};
    var keys = Object.keys(proc.env || {});
    for (var i = 0; i < keys.length; i++) env[keys[i]] = proc.env[keys[i]];
    env.QUENCH_WORKER = '1';
    env.QUENCH_WORKER_DATA = JSON.stringify(options.workerData === undefined ? null : options.workerData);
    var result = cp.spawnSync(proc.execPath, [filename], { env: env, cwd: options.cwd });
    var lines = String(result.stdout || '').split('\n');
    for (var j = 0; j < lines.length; j++) {
      var marker = '__QUENCH_WORKER_MESSAGE__';
        try { self.emit('message', JSON.parse(lines[j].slice(marker.length))); } catch (_) {}
      }
    self.exited = true;
    self.emit('exit', result.status === null ? 1 : result.status);
    if (result.status !== 0 && result.stderr) self.emit('error', new Error(String(result.stderr)));
    self.postMessage = function () {};
    self.terminate = function () { return Promise.resolve(result.status === null ? 1 : result.status); };
    self.ref = self.unref = function () { return self; };
    return self;
  },
  receiveMessageOnPort: function () { return undefined; },
  markAsUncloneable: function () {}, markAsUntransferable: function () {},
  setEnvironmentData: function () {}, getEnvironmentData: function () { return undefined; }
};
module.exports = api;
