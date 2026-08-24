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
  target._takePending = function (name) {
    if (!pending) return undefined;
    for (var p = 0; p < pending.length; p++) {
      if (pending[p][0] === name) {
        var item = pending[p][1];
        pending.splice(p, 1);
        return item;
      }
    }
    return undefined;
  };
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
  target.addListener = target.on;
  target.off = target.removeListener;
  target.emit = function (name) {
    var a = (listeners[name] || []).slice(), args = Array.prototype.slice.call(arguments, 1);
    if (pending && a.length === 0) pending.push([name, args]);
    for (var i = 0; i < a.length; i++) a[i].apply(target, args);
    return a.length > 0;
  };
  return target;
}
var parentPort = workerEnv ? emitter({
  _closed: false,
  postMessage: function (value) {
    if (this._closed) throw new Error('Cannot post message after closing parentPort');
    console.log('__QUENCH_WORKER_MESSAGE__' + JSON.stringify(value));
  },
  close: function () {
    if (!this._closed) {
      this._closed = true;
      this.emit('close');
    }
  }
}) : null;
function cloneMessage(value) {
  if (value && typeof value.postMessage === 'function' && typeof value.close === 'function') {
    return value;
  }
  if (value instanceof Uint8Array) return new Uint8Array(value);
  if (Array.isArray(value)) {
    var array = [];
    for (var i = 0; i < value.length; i++) array.push(cloneMessage(value[i]));
    return array;
  }
  if (value && typeof value === 'object') {
    var object = {};
    var keys = Object.keys(value);
    for (var j = 0; j < keys.length; j++) object[keys[j]] = cloneMessage(value[keys[j]]);
    return object;
  }
  return value;
}
function messagePort() {
var port = emitter({ _queueEvents: true, _peer: null, close: function (callback) {
  if (this._closed) return;
  this._closed = true;
  this.emit('close');
  if (typeof callback === 'function') callback.call(this);
  if (this._peer && !this._peer._closed) {
    this._peer._closed = true;
    this._peer.emit('close');
  }
} });
  var onmessage = null;
  var onmessageListener = null;
  port.onmessage = null;
  Object.defineProperty(port, 'onmessage', {
    configurable: true,
    enumerable: true,
    get: function () { return onmessage; },
    set: function (fn) {
      if (onmessageListener) port.removeListener('message', onmessageListener);
      onmessage = typeof fn === 'function' ? fn : null;
      onmessageListener = onmessage ? function (value, ports) {
        onmessage.call(port, { data: value, target: port, ports: ports || [] });
      } : null;
      if (onmessageListener) port.on('message', onmessageListener);
    }
  });
  port.postMessage = function (value) {
    var cloned = cloneMessage(value);
    var transferredPorts = [];
    var transfer = arguments[1];
    var options = transfer && typeof transfer === 'object' && !Array.isArray(transfer) &&
      Object.prototype.hasOwnProperty.call(transfer, 'transfer');
    if (options) {
      transfer = transfer.transfer;
    }
    if (!options && transfer && typeof transfer === 'object' &&
        !Array.isArray(transfer) && typeof transfer[Symbol.iterator] !== 'function') {
      transfer = [];
    }
    if (options && transfer !== undefined && transfer !== null &&
        typeof transfer !== 'object' && typeof transfer[Symbol.iterator] !== 'function') {
      throw Object.assign(new TypeError('Optional options.transfer argument must be an iterable'), {
        code: 'ERR_INVALID_ARG_TYPE'
      });
    }
    if (options && transfer === null) {
      throw Object.assign(new TypeError('Optional options.transfer argument must be an iterable'), {
        code: 'ERR_INVALID_ARG_TYPE'
      });
    }
    if (transfer === null || transfer === undefined) transfer = [];
    if (typeof transfer !== 'object') {
      throw Object.assign(new TypeError('Optional transferList argument must be an iterable'), {
        code: 'ERR_INVALID_ARG_TYPE'
      });
    }
    if (!Array.isArray(transfer)) {
      if (typeof transfer[Symbol.iterator] !== 'function') {
        throw Object.assign(new TypeError('Optional transferList argument must be an iterable'), {
          code: 'ERR_INVALID_ARG_TYPE'
        });
      }
      var iterator = transfer[Symbol.iterator]();
      if (!iterator || typeof iterator.next !== 'function') {
        throw Object.assign(new TypeError('Optional transferList argument must be an iterable'), {
          code: 'ERR_INVALID_ARG_TYPE'
        });
      }
      var iterable = [];
      var step = iterator.next();
      while (!step.done) {
        iterable.push(step.value);
        step = iterator.next();
      }
      transfer = iterable;
    }
    if (transfer && typeof transfer.length === 'number') {
      for (var t = 0; t < transfer.length; t++) {
        var item = transfer[t];
        if (item && typeof item.postMessage === 'function' && typeof item.close === 'function') {
          transferredPorts.push(item);
          continue;
        }
        var buffer = item instanceof ArrayBuffer ? item : item && item.buffer;
        if (buffer instanceof ArrayBuffer) {
          try { buffer.transfer(); } catch (_) {
            throw Object.assign(new Error('ArrayBuffer is not transferable'), {
              name: 'DataCloneError', code: 25
            });
          }
        }
      }
    }
    if (!this._closed && this._peer && !this._peer._closed) {
      this._peer.emit('message', cloned, transferredPorts);
    }
  };
  port.start = function () { return this; };
  port.addEventListener = function (name, fn) {
    return this.on(name, function (value) {
      fn.call(port, {
        type: name,
        detail: value,
        data: name === 'message' ? value : undefined,
        ports: name === 'message' ? (arguments[1] || []) : undefined,
        target: port
      });
    });
  };
  port.removeEventListener = function (name, fn) {
    return this.removeListener(name, fn);
  };
  Object.setPrototypeOf(port, messagePort.prototype);
  return port;
}
var environmentData = {};
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
  receiveMessageOnPort: function (port) {
    if (!port || typeof port._takePending !== 'function') return undefined;
    var args = port._takePending('message');
    return args ? { message: args[0] } : undefined;
  },
  markAsUncloneable: function () {}, markAsUntransferable: function () {},
  setEnvironmentData: function (key, value) { environmentData[String(key)] = value; },
  getEnvironmentData: function (key) {
    return Object.prototype.hasOwnProperty.call(environmentData, String(key)) ? environmentData[String(key)] : undefined;
  }
};
module.exports = api;
