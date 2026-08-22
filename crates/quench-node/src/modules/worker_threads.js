// Worker adapter: execute worker files in a separate quench-node process.
// The synchronous launch is intentional: it gives the small embedding a real
// isolated VM while preserving deterministic event delivery to the parent.
var proc = process;
var workerArgs = proc.argv || [];
var workerEnv = proc.env && proc.env.QUENCH_WORKER;
var workerArgData = null;
for (var wa = 0; wa < workerArgs.length; wa++) {
  if (workerArgs[wa] === '--quench-worker') workerEnv = '1';
  if (String(workerArgs[wa]).indexOf('--quench-worker-data=') === 0) workerArgData = String(workerArgs[wa]).slice(21);
}
var workerData = workerEnv ? JSON.parse(workerArgData || (proc.env && proc.env.QUENCH_WORKER_DATA) || 'null') : null;
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
function messagePort() {
  var port = emitter({ _queueEvents: true, _peer: null, close: function () { if (!this._closed) { this._closed = true; this.emit('close'); } } });
  port.postMessage = function (value) {
    if (!this._closed && this._peer && !this._peer._closed) this._peer.emit('message', value);
  };
  port.start = function () { return this; };
  return port;
}
function MessageChannel() {
  this.port1 = messagePort();
  this.port2 = messagePort();
  this.port1._peer = this.port2;
  this.port2._peer = this.port1;
}
var api = {
  isMainThread: !workerEnv, threadId: workerEnv ? 1 : 0, workerData: workerData,
  parentPort: parentPort, MessageChannel: MessageChannel, MessagePort: messagePort,
    Worker: function Worker(filename, options) {
      if (!(typeof filename === 'string' || (filename && typeof filename.toString === 'function'))) {
        throw new TypeError('The "filename" argument must be a string');
      }
      var self = emitter({ threadId: 1, exited: false, _queueEvents: true });
      options = options || {};
      var cp = require('child_process');
      var env = {};
      var keys = Object.keys(proc.env || {});
      for (var i = 0; i < keys.length; i++) env[keys[i]] = proc.env[keys[i]];
      env.QUENCH_WORKER = '1';
      var data = options.workerData === undefined ? null : options.workerData;
      env.QUENCH_WORKER_DATA = JSON.stringify(data);
      var result = cp.spawnSync(proc.execPath, [String(filename), '--quench-worker', '--quench-worker-data=' + JSON.stringify(data)], { env: env, cwd: options.cwd });
      var status = result && result.status === null ? 1 : (result ? result.status : 1);
      var output = result && result.stdout && typeof result.stdout.toString === 'function' ? result.stdout.toString() : String(result && result.stdout || '');
      var lines = output.split('\n');
      for (var j = 0; j < lines.length; j++) {
        var marker = '__QUENCH_WORKER_MESSAGE__';
        if (lines[j].indexOf(marker) !== 0) continue;
        try { self.emit('message', JSON.parse(lines[j].slice(marker.length))); } catch (_) {}
      }
      self.exited = true;
      self.emit('online');
      self.emit('exit', status);
      if (status !== 0 && result && result.stderr) self.emit('error', new Error(String(result.stderr)));
      self.postMessage = function () {};
      self.terminate = function () { self.exited = true; return Promise.resolve(status); };
      self.ref = self.unref = function () { return self; };
      return self;
    },
  receiveMessageOnPort: function () { return undefined; },
  markAsUncloneable: function () {}, markAsUntransferable: function () {},
  setEnvironmentData: function () {}, getEnvironmentData: function () { return undefined; }
};
module.exports = api;
