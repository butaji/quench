//! The JavaScript surface of `worker_threads` and the global `MessageChannel`/`MessagePort`,
//! over the native port transport and worker threads (`super::ports`, `super::thread`).

/// `(native, EventEmitter, path, fileURLToPath, Readable) => api`: builds the port and worker classes once per VM.
/// `api.createPort(id)` wraps a native port, `api.dispatch`/`api.closed`/`api.workerEvent`
/// deliver event-loop events, and `api.module(context)` builds the `worker_threads` exports.
pub(crate) const MESSAGING_JS: &str = quench_js_check::checked_js!(
    r#"(native, EventEmitter, path, fileURLToPath, Readable) => {
'use strict';
const portIds = new WeakMap();
const nodeListeners = Symbol('kNodeListeners');
const hybridDispatch = Symbol('kHybridDispatch');
const maxListeners = Symbol('kMaxListeners');
const startedReading = Symbol('kStartedReading');
const onHandlers = new WeakMap();
let constructing = false;

const codedError = (Base, code, message) => {
  const error = new Base(message);
  Object.defineProperty(error, 'code', { value: code, writable: true, enumerable: false, configurable: true });
  return error;
};
const dataCloneError = (message) => new DOMException(message, 'DataCloneError');

// Node's NodeEventTarget: EventTarget plus EventEmitter-style methods. Node-style
// listeners receive the event's value; DOM listeners receive the Event.
class NodeEventTarget extends EventTarget {
  constructor() {
    super();
    this[nodeListeners] = new Map();
    this[maxListeners] = EventEmitter.defaultMaxListeners;
  }
  setMaxListeners(n) { this[maxListeners] = n; return this; }
  getMaxListeners() { return this[maxListeners]; }
  eventNames() { return [...this[nodeListeners].keys()].filter((type) => this[nodeListeners].get(type).length > 0); }
  listenerCount(type) { return (this[nodeListeners].get(type) || []).length; }
  addEventListener(type, listener, options) { addRecord(this, String(type), listener, options, false); }
  removeEventListener(type, listener) { removeRecord(this, String(type), listener, false); }
  addListener(type, listener, options) { addRecord(this, String(type), listener, options, true); return this; }
  on(type, listener, options) { return this.addListener(type, listener, options); }
  once(type, listener, options) { return this.addListener(type, listener, { ...options, once: true }); }
  removeListener(type, listener) { removeRecord(this, String(type), listener, true); return this; }
  off(type, listener) { return this.removeListener(type, listener); }
  removeAllListeners(type) {
    const types = type === undefined ? [...this[nodeListeners].keys()] : [String(type)];
    for (const name of types) {
      for (const record of [...(this[nodeListeners].get(name) || [])]) removeRecord(this, name, record.listener, record.nodeStyle);
    }
    return this;
  }
  emit(type, value) {
    this[hybridDispatch](value, String(type), () => new Event(String(type)));
    return true;
  }
  dispatchEvent(event) {
    if (!(event instanceof Event)) throw codedError(TypeError, 'ERR_INVALID_ARG_TYPE', 'The "event" argument must be an instance of Event.');
    this[hybridDispatch](event, event.type, () => event);
    return !event.defaultPrevented;
  }
  [hybridDispatch](value, type, createEvent) {
    const records = this[nodeListeners].get(type);
    if (records === undefined || records.length === 0) return;
    let event;
    for (const record of [...records]) {
      if (!records.includes(record)) continue;
      if (record.once) removeRecord(this, type, record.listener, record.nodeStyle);
      const argument = record.nodeStyle ? value : (event ??= createEvent());
      const listener = record.listener;
      try {
        if (typeof listener === 'function') listener.call(this, argument);
        else listener.handleEvent(argument);
      } catch (error) {
        process.nextTick(() => { throw error; });
      }
      if (event?._quenchImmediatePropagationStopped) break;
    }
  }
}
function addRecord(target, type, listener, options, nodeStyle) {
  if (listener == null || (typeof listener !== 'function' && typeof listener?.handleEvent !== 'function')) {
    if (nodeStyle) throw codedError(TypeError, 'ERR_INVALID_ARG_TYPE', 'The "listener" argument must be of type function.');
    return;
  }
  const records = target[nodeListeners].get(type) || [];
  if (!nodeStyle && records.some((record) => record.listener === listener && !record.nodeStyle)) return;
  records.push({ listener, nodeStyle, once: Boolean(options?.once) });
  target[nodeListeners].set(type, records);
  if (records.length === 1) target.constructor?.[Symbol.for('quench.newListener')]?.(target, type);
}
function removeRecord(target, type, listener, nodeStyle) {
  const records = target[nodeListeners].get(type);
  if (!records) return;
  const index = records.findIndex((record) => record.listener === listener && record.nodeStyle === nodeStyle);
  if (index < 0) return;
  records.splice(index, 1);
  if (records.length === 0) target.constructor?.[Symbol.for('quench.removedListener')]?.(target, type);
}

const id = (port) => {
  const value = portIds.get(port);
  if (value === undefined) throw new TypeError('Illegal invocation');
  return value;
};

class MessagePort extends NodeEventTarget {
  constructor() {
    if (!constructing) throw codedError(TypeError, 'ERR_CONSTRUCT_CALL_INVALID', 'Constructor cannot be called');
    super();
  }
  postMessage(value, transferList) {
    const port = id(this);
    let transfer = [];
    if (transferList !== undefined) {
      if (transferList === null || typeof transferList[Symbol.iterator] !== 'function') {
        if (typeof transferList === 'object' && transferList !== null && transferList.transfer !== undefined) {
          transferList = transferList.transfer;
        }
        if (transferList === null || typeof transferList?.[Symbol.iterator] !== 'function') {
          throw codedError(TypeError, 'ERR_INVALID_ARG_TYPE', 'Optional transferList argument must be an iterable');
        }
      }
      transfer = [...transferList];
    }
    const ports = [];
    const buffers = [];
    for (const item of transfer) {
      if (portIds.has(item)) {
        if (item === this) throw dataCloneError('Transfer list contains source port');
        ports.push(item);
      } else {
        buffers.push(item);
      }
    }
    native.post(port, value, buffers, ports, ports.map((item) => portIds.get(item)));
  }
  start() { native.start(id(this)); }
  close() { native.close(id(this)); }
  ref() { native.ref(id(this), true); }
  unref() { native.ref(id(this), false); }
  hasRef() { return native.hasRef(id(this)); }
  get onmessage() { return onHandlers.get(this)?.message ?? null; }
  set onmessage(handler) { setHandler(this, 'message', handler); }
  get onmessageerror() { return onHandlers.get(this)?.messageerror ?? null; }
  set onmessageerror(handler) { setHandler(this, 'messageerror', handler); }
  static [Symbol.for('quench.newListener')](port, type) {
    if (type === 'message') { port[startedReading] = true; port.ref(); port.start(); }
  }
  static [Symbol.for('quench.removedListener')](port, type) {
    if (type === 'message' && port[startedReading]) { port[startedReading] = false; port.unref(); }
  }
}
for (const name of ['onmessage', 'onmessageerror']) {
  Object.defineProperty(MessagePort.prototype, name, { enumerable: true });
}
Object.defineProperty(MessagePort, 'name', { value: 'MessagePort' });
function setHandler(port, type, handler) {
  const handlers = onHandlers.get(port) || {};
  if (handlers[type]) port.removeEventListener(type, handlers[type]);
  handlers[type] = typeof handler === 'function' ? handler : null;
  onHandlers.set(port, handlers);
  if (handlers[type]) port.addEventListener(type, handlers[type]);
}

class MessageChannel {
  constructor() {
    const [port1, port2] = native.channel();
    Object.defineProperty(this, 'port1', { value: port1, enumerable: true, configurable: false, writable: false });
    Object.defineProperty(this, 'port2', { value: port2, enumerable: true, configurable: false, writable: false });
  }
}

const emitInit = (resource, type) => globalThis['\0quench:async_hooks:emit_init']?.(resource, type);

function createPort(portId) {
  constructing = true;
  let port;
  try { port = new MessagePort(); } finally { constructing = false; }
  portIds.set(port, portId);
  emitInit(port, 'MESSAGEPORT');
  return port;
}

function receiveMessageOnPort(port) {
  if (!portIds.has(port)) throw codedError(TypeError, 'ERR_INVALID_ARG_TYPE', 'The "port" argument must be a MessagePort instance');
  return native.receive(portIds.get(port));
}

const SHARE_ENV = Symbol.for('nodejs.worker_threads.SHARE_ENV');
const workerPorts = new WeakMap();
const threadIds = new WeakMap();
const workerStdio = new WeakMap();
const workerRefs = new WeakMap();
const workerExits = new WeakMap();

class Worker extends EventEmitter {
  constructor(filename, options = {}) {
    super();
    if (options === null || typeof options !== 'object') {
      throw codedError(TypeError, 'ERR_INVALID_ARG_TYPE', 'The "options" argument must be of type object.');
    }
    const isEval = Boolean(options.eval);
    if (filename instanceof URL) {
      if (filename.protocol !== 'file:') throw codedError(TypeError, 'ERR_WORKER_UNSUPPORTED_OPERATION', 'Unsupported Worker URL protocol');
      filename = fileURLToPath(filename);
    } else if (typeof filename !== 'string') {
      throw codedError(TypeError, 'ERR_INVALID_ARG_TYPE', 'The "filename" argument must be of type string or an instance of URL.');
    } else if (!isEval) {
      if (!path.isAbsolute(filename) && !/^\.\.?[\\/]/.test(filename)) {
        throw codedError(TypeError, 'ERR_WORKER_PATH',
          `The worker script or module filename must be an absolute path or a relative path starting with './' or '../'. Received "${filename}"`);
      }
      filename = path.resolve(filename);
    }
    if (options.argv !== undefined && !Array.isArray(options.argv)) {
      throw codedError(TypeError, 'ERR_INVALID_ARG_TYPE', 'The "options.argv" property must be an instance of Array.');
    }
    if (options.execArgv !== undefined && !Array.isArray(options.execArgv)) {
      throw codedError(TypeError, 'ERR_INVALID_ARG_TYPE', 'The "options.execArgv" property must be an instance of Array.');
    }
    if (options.env !== undefined && options.env !== null && options.env !== SHARE_ENV && typeof options.env !== 'object') {
      throw codedError(TypeError, 'ERR_INVALID_ARG_TYPE',
        `The "options.env" property must be of type object or one of undefined, null, or worker_threads.SHARE_ENV. Received type ${typeof options.env} (${String(options.env)})`);
    }
    const argv = (options.argv || []).map(String);
    const execArgv = options.execArgv === undefined ? undefined : options.execArgv.map(String);
    const source = options.env === SHARE_ENV || options.env == null ? process.env : options.env;
    const env = Object.entries(source ?? {}).map(([key, value]) => [key, String(value)]);
    let transfer = [];
    if (options.transferList !== undefined) {
      if (options.transferList === null || typeof options.transferList[Symbol.iterator] !== 'function') {
        throw codedError(TypeError, 'ERR_INVALID_ARG_TYPE', 'Optional transferList argument must be an iterable');
      }
      transfer = [...options.transferList];
    }
    const ports = transfer.filter((item) => portIds.has(item));
    const buffers = transfer.filter((item) => !portIds.has(item));
    const [threadId, port] = native.workerStart(this, String(filename), isEval, argv, execArgv, env,
      options.workerData, buffers, ports, ports.map((item) => portIds.get(item)));
    threadIds.set(this, threadId);
    workerPorts.set(this, port);
    const stdio = { stdout: new Readable({ read() {} }), stderr: new Readable({ read() {} }) };
    if (!options.stdout) stdio.stdout.on('data', (chunk) => process.stdout.write(chunk));
    if (!options.stderr) stdio.stderr.on('data', (chunk) => process.stderr.write(chunk));
    workerStdio.set(this, stdio);
    workerRefs.set(this, true);
    emitInit({ hasRef: () => workerRefs.get(this) === true }, 'WORKER');
    port.on('message', (value) => this.emit('message', value));
    port.on('messageerror', (error) => this.emit('messageerror', error));
    port.unref();
  }
  postMessage(value, transferList) { workerPorts.get(this).postMessage(value, transferList); }
  ref() { workerRefs.set(this, true); native.workerRef(this, true); }
  unref() { workerRefs.set(this, false); native.workerRef(this, false); }
  terminate(callback) {
    const exit = workerExits.get(this) ?? new Promise((resolve) => {
      if (native.workerExited(this)) { resolve(undefined); return; }
      this.once('exit', resolve);
    });
    workerExits.set(this, exit);
    native.workerTerminate(this);
    if (typeof callback === 'function') exit.then((code) => callback(null, code));
    return exit;
  }
  get threadId() { return threadIds.get(this); }
  get threadName() { return 'WorkerThread'; }
  get resourceLimits() { return {}; }
  get stdin() { return null; }
  get stdout() { return workerStdio.get(this).stdout; }
  get stderr() { return workerStdio.get(this).stderr; }
  getHeapSnapshot() { return Promise.reject(codedError(Error, 'ERR_WORKER_NOT_RUNNING', 'Worker instance not running')); }
  getHeapStatistics() { return Promise.reject(codedError(Error, 'ERR_WORKER_NOT_RUNNING', 'Worker instance not running')); }
  cpuUsage() { return Promise.reject(codedError(Error, 'ERR_WORKER_NOT_RUNNING', 'Worker instance not running')); }
  startCpuProfile() { return Promise.reject(codedError(Error, 'ERR_WORKER_NOT_RUNNING', 'Worker instance not running')); }
  startHeapProfile() { return Promise.reject(codedError(Error, 'ERR_WORKER_NOT_RUNNING', 'Worker instance not running')); }
}

function workerEvent(worker, kind, value) {
  if (kind === 'online') worker.emit('online');
  else if (kind === 'error') worker.emit('error', value);
  else if (kind === 'stdout' || kind === 'stderr') {
    workerStdio.get(worker)[kind].push(Buffer.from(value.buffer, value.byteOffset, value.byteLength));
  } else if (kind === 'exit') {
    workerPorts.get(worker)?.close();
    const stdio = workerStdio.get(worker);
    stdio.stdout.push(null);
    stdio.stderr.push(null);
    workerRefs.set(worker, false);
    // An exited worker no longer has a thread.
    threadIds.set(worker, -1);
    worker.emit('exit', value);
  }
}

const environmentData = new Map();
function module(context) {
  return {
    isInternalThread: false,
    isMainThread: context.isMainThread,
    MessagePort,
    MessageChannel,
    markAsUncloneable: context.markAsUncloneable,
    markAsUntransferable: context.markAsUntransferable,
    isMarkedAsUntransferable: context.isMarkedAsUntransferable,
    moveMessagePortToContext() {
      throw codedError(Error, 'ERR_METHOD_NOT_IMPLEMENTED', 'The moveMessagePortToContext() method is not implemented');
    },
    receiveMessageOnPort,
    resourceLimits: {},
    postMessageToThread() {
      return Promise.reject(codedError(Error, 'ERR_WORKER_MESSAGING_FAILED', 'Cannot find the destination thread or listener'));
    },
    threadId: context.threadId,
    threadName: context.isMainThread ? 'main' : 'WorkerThread',
    SHARE_ENV,
    Worker,
    parentPort: context.parentPort,
    workerData: context.workerData,
    BroadcastChannel: globalThis.BroadcastChannel,
    setEnvironmentData(key, value) {
      if (value === undefined) environmentData.delete(key); else environmentData.set(key, value);
    },
    getEnvironmentData(key) { return environmentData.get(key); },
    locks: globalThis.navigator?.locks,
  };
}

return {
  MessagePort,
  MessageChannel,
  createPort,
  dispatch(port, value, ports) {
    port[hybridDispatch](value, 'message', () => new MessageEvent('message', { data: value, ports }));
  },
  closed(port) {
    port[hybridDispatch](undefined, 'close', () => new Event('close'));
  },
  workerEvent,
  module,
};
}"#
);
