use super::net_connect_error::TcpConnectFailure;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, Value};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc::{self, Receiver, TryRecvError};

pub(crate) mod encrypted;

const FIRST_TRANSPORT_ID: u64 = 1;
const TCP_READ_CHUNK: usize = 16 * 1024;
const TCP_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const CALLABLE_FACADE: &str = quench_js_check::checked_js!(
    r#"(ip, ipv4, ipv6, getFamily, setFamily, getTimeout, setTimeout) => ({
  isIP: function isIP(value) { 'use strict'; return ip(value); },
  isIPv4: function isIPv4(value) { 'use strict'; return ipv4(value); },
  isIPv6: function isIPv6(value) { 'use strict'; return ipv6(value); },
  getDefaultAutoSelectFamily: function getDefaultAutoSelectFamily() { 'use strict'; return getFamily(); },
  setDefaultAutoSelectFamily: function setDefaultAutoSelectFamily(value) { 'use strict'; return setFamily(value); },
  getDefaultAutoSelectFamilyAttemptTimeout: function getDefaultAutoSelectFamilyAttemptTimeout() { 'use strict'; return getTimeout(); },
  setDefaultAutoSelectFamilyAttemptTimeout: function setDefaultAutoSelectFamilyAttemptTimeout(value) { 'use strict'; return setTimeout(value); },
})"#
);

const NET_MODULE_FACTORY: &str = r#"((Duplex, connectOperation, writeOperation, endOperation, destroyOperation, encodingOperation, listenOperation, closeOperation, addressOperation, accountWriteOperation, endpointOperation, referenceOperation, socketReferenceOperation, unrefTimeoutOperation, socketOptionsOperation, serverOptionsOperation, initEndOperation, onReadableStreamEnd, initServerEmitterOperation, initSocketOptionsOperation, setNoDelay, setKeepAlive, initServerOptionsOperation, getConnections, initializeRead, readSocket, endStreamOperation, normalizedArgsSymbol, ownerSymbol, asyncIdSymbol, pipeBinding, streamBinding) => {
  class Socket extends Duplex {
    constructor(options = {}) {
      options = { ...socketOptionsOperation(options, false) };
      // Child-process and cluster extra stdio pipes are passed as raw file
      // descriptors. Node's net.Socket adopts those descriptors with a native
      // Pipe handle; the TCP facade must do the same so fd-backed sockets can
      // participate in normal Readable and Writable flow.
      if (options.handle == null && options.fd !== undefined) {
        const permission = process.permission;
        const audit = (process.execArgv || []).includes('--permission-audit');
        if (permission && !audit && !permission.has('net') && options.fd > 2) {
          throw Object.assign(
            new Error('Access to this API has been restricted. Use --allow-net to manage permissions.'),
            { code: 'ERR_ACCESS_DENIED', permission: 'Net' },
          );
        }
        const { Pipe, constants } = pipeBinding;
        const handle = new Pipe(constants.SOCKET);
        const status = handle.open(options.fd);
        if (status < 0) {
          handle.close();
          const error = new Error(`open fd ${options.fd} failed`);
          error.errno = status;
          throw error;
        }
        Object.defineProperty(handle, '__quenchPipeHandle', { value: true });
        options.handle = handle;
      }
      super({
        ...options, autoDestroy: true, allowHalfOpen: options.allowHalfOpen ?? false, emitClose: false,
        write(chunk, encoding, callback) {
          const handle = this._handle;
          if (handle?.__quenchJSStream) {
            try {
              handle.onwrite({ oncomplete: callback }, [chunk]);
            } catch (error) {
              callback(error);
            }
            return;
          }
          if (handle?.__quenchPipeHandle) {
            try {
              const { WriteWrap } = streamBinding;
              const { streamBaseState, kLastWriteWasAsync, kLastWriteErr } = streamBinding;
              const request = new WriteWrap();
              request.oncomplete = (status) => callback(status < 0
                ? Object.assign(new Error(`write failed with status ${status}`), { errno: status })
                : undefined);
              const status = handle.writeBuffer(request, chunk);
              if (status < 0) {
                callback(Object.assign(new Error(`write failed with status ${status}`), { errno: status }));
              } else if (streamBaseState[kLastWriteWasAsync] === 0) {
                const writeError = streamBaseState[kLastWriteErr];
                queueMicrotask(() => callback(writeError < 0
                  ? Object.assign(new Error(`write failed with status ${writeError}`), { errno: writeError })
                  : undefined));
              }
            } catch (error) {
              callback(error);
            }
            return;
          }
          try { writeOperation(this, chunk, callback); } catch (error) { callback(error); }
        },
        final(callback) { try { endOperation(this); callback(); } catch (error) { callback(error); } },
        destroy(error, callback) {
          const handle = this._handle;
          if (handle?.__quenchJSStream) {
            // Node closes an adopted JSStream handle before completing the
            // Socket destroy callback. That close gives JSStreamSocket a
            // chance to cancel pending write/shutdown requests with
            // UV_ECANCELED. Clear the owner slot first, as net.js does.
            try {
              handle.close(() => {
                try { destroyOperation(this, error, callback); } catch (failure) { callback(failure); }
              });
              // close() must first let JSStreamSocket capture its handle for
              // the deferred request cancellation, then the Socket drops its
              // public owner slot as Node's closeSocketHandle() does.
              this._handle = null;
            } catch (failure) {
              this._handle = null;
              callback(failure);
            }
            return;
          }
          if (handle?.__quenchPipeHandle) {
            try {
              handle.close(() => {
                this._handle = null;
                try { destroyOperation(this, error, callback); } catch (failure) { callback(failure); }
              });
              this._handle = null;
            } catch (failure) {
              this._handle = null;
              callback(failure);
            }
            return;
          }
          try { destroyOperation(this, error, callback); } catch (failure) { callback(failure); }
        },
      });
      initEndOperation(this, options, onReadableStreamEnd);
      this._encoding = null;
      this._handle = options.handle || null;
      this[asyncIdSymbol] = this._handle && typeof this._handle.getAsyncId === 'function'
        ? this._handle.getAsyncId() : -1;
      if (this._handle && this._handle.__quenchJSStream) {
        // Node's initSocketHandle() associates the adopted stream handle with
        // its owning Socket. JSStream callbacks use this symbol to reach back
        // into the JSStreamSocket adapter for reads, writes, and shutdowns.
        this._handle[ownerSymbol] = this;
        this._handle.__quenchOwner = this;
      }
      if (this._handle?.__quenchPipeHandle) {
        this._handle.onread = (buffer) => {
          if (buffer === null) this.push(null);
          else this.push(Buffer.from(buffer));
        };
      } else if (this._handle && !this._handle.__quenchJSStream) {
        // Node's initSocketHandle installs onStreamRead on adopted handles.
        // Custom handles (including the small handles used by embedders and
        // tests) invoke this callback with stream_wrap's shared read state.
        this._handle[ownerSymbol] = this;
        this._handle.onread = function onStreamRead(arrayBuffer) {
          const { streamBaseState, kReadBytesOrError, kArrayBufferOffset } =
            streamBinding;
          const nread = streamBaseState[kReadBytesOrError];
          const socket = this[ownerSymbol];
          if (nread > 0 && !socket.destroyed) {
            const offset = streamBaseState[kArrayBufferOffset];
            socket.push(Buffer.from(arrayBuffer, offset, nread));
          } else if (nread === -4095) {
            socket.push(null);
            socket.read(0);
          } else if (nread < 0) {
            const error = new Error(`read ${nread}`);
            error.errno = nread;
            error.syscall = 'read';
            socket.destroy(error);
          }
        };
      }
      initSocketOptionsOperation(this, options);
      this.connecting = false;
      this.pending = true;
      this.bytesRead = 0;
      this.bytesWritten = 0;
      this._quenchTimeout = 0;
      this._quenchTimeoutTimer = null;
      this.on('close', () => {
        this.pending = true;
        if (this._quenchTimeoutTimer !== null) clearTimeout(this._quenchTimeoutTimer);
        this._quenchTimeoutTimer = null;
      });
      initializeRead(this, options);
    }
    emit(event, ...args) {
      if (event === 'connect') {
        this.connecting = false;
        this.pending = false;
        this._quenchResetTimeout();
        return super.emit(event, ...args);
      }
      if (event === 'close') { this.pending = true; if (this._quenchTimeoutTimer !== null) clearTimeout(this._quenchTimeoutTimer); this._quenchTimeoutTimer = null; }
      return super.emit(event, ...args);
    }
    setEncoding(encoding) { this._encoding = String(encoding); encodingOperation(this, this._encoding); super.setEncoding(encoding); return this; }
    get readyState() {
      if (this.connecting) return 'opening';
      if (this.destroyed || this.closed) return 'closed';
      if (this.readable && this.writable) return 'open';
      if (this.readable) return 'readOnly';
      if (this.writable) return 'writeOnly';
      return 'closed';
    }
    get bufferSize() { return this.writableLength || 0; }
    _unrefTimer() {
      // Match Node's loose null check: socket teardown can leave `_parent`
      // undefined, and each parent in the chain may own a refreshed timeout.
      for (let socket = this; socket != null; socket = socket._parent) {
        if (socket._quenchTimeoutTimer !== null) socket._quenchTimeoutTimer.refresh();
      }
    }
    address() { return this._getsockname(); }
    get writableLength() {
      const streamLength = this._writableState?.length || 0;
      const nativeLength = this._handle?.writeQueueSize || 0;
      const httpRequestLength = this.__quenchHttpRequestWritePending || 0;
      return Math.max(streamLength, nativeLength, httpRequestLength);
    }
    setTimeout(timeout, callback) {
      if (typeof timeout !== 'number') {
        throw Object.assign(new TypeError(`The "msecs" argument must be of type number. Received ${String(timeout)}`), { code: 'ERR_INVALID_ARG_TYPE' });
      }
      if (!Number.isFinite(timeout) || timeout < 0) {
        throw Object.assign(new RangeError(`The value of "msecs" is out of range. It must be >= 0. Received ${String(timeout)}`), { code: 'ERR_OUT_OF_RANGE' });
      }
      if (callback !== undefined && typeof callback !== 'function') {
        throw Object.assign(new TypeError(`The "callback" argument must be of type function. Received ${String(callback)}`), { code: 'ERR_INVALID_ARG_TYPE' });
      }
      if (typeof callback === 'function') this.once('timeout', callback);
      this.timeout = timeout;
      this._quenchTimeout = timeout;
      this._quenchResetTimeout();
      return this;
    }
    _quenchResetTimeout() {
      if (this._quenchTimeoutTimer !== null) clearTimeout(this._quenchTimeoutTimer);
      this._quenchTimeoutTimer = null;
      if (this._quenchTimeout > 0 && !this.destroyed && !this.connecting) {
        this._quenchTimeoutTimer = setTimeout(() => this.emit('timeout'), this._quenchTimeout);
        unrefTimeoutOperation(this._quenchTimeoutTimer);
      }
    }
    write(chunk, encoding, callback, internalAccounted = false) {
      if (typeof encoding === 'function') { callback = encoding; encoding = undefined; }
      this._quenchResetTimeout();
      accountWriteOperation(this, chunk, encoding, internalAccounted);
      return super.write(chunk, encoding, callback, internalAccounted);
    }
    end(chunk, encoding, callback) {
      if (typeof chunk === 'function') { callback = chunk; chunk = undefined; encoding = undefined; }
      else if (typeof encoding === 'function') { callback = encoding; encoding = undefined; }
      return endStreamOperation(this, chunk, encoding, callback);
    }
    connect(port, host, callback) {
      if (Array.isArray(port) && port[normalizedArgsSymbol] === true) {
        callback = port[1] || callback;
        port = port[0];
        host = undefined;
      }
      const plan = endpointOperation(port, host, callback, true);
      port = plan.address; host = plan.host; callback = plan.callback;
      const options = plan.options;
      const blockList = options?.blockList;
      if (options?.hints !== undefined && !Number.isInteger(options.hints)) {
        const error = new TypeError(`The argument 'hints' is invalid. Received ${String(options.hints)}`);
        error.code = 'ERR_INVALID_ARG_VALUE'; throw error;
      }
      // connect(options) applies TCP_NODELAY and keep-alive once the socket is
      // connected. Cache the requested values on the socket so
      // apply_connected() can project them onto the native transport.
      if (options?.noDelay !== undefined) this.setNoDelay(options.noDelay);
      if (options?.keepAlive !== undefined) this.setKeepAlive(options.keepAlive, options.keepAliveInitialDelay);
      if (typeof callback === 'function') this.once('connect', callback);
      const permission = process.permission;
      const permissionAudit = (process.execArgv || []).includes('--permission-audit');
      if (permission && !permissionAudit && !permission.has('net', `${host}:${port}`)) {
        const numericHost = typeof host === 'string' &&
          (host.includes(':') || /^\d+(?:\.\d+){3}$/.test(host));
        queueMicrotask(() => {
          this.connecting = false;
          this.pending = false;
          const syscall = numericHost ? 'connect' : 'getaddrinfo';
          const detail = numericHost ? 'Access to this API has been restricted. Use --allow-net to manage permissions.' : String(host);
          this.emit('error', Object.assign(
            new Error(`${syscall} ERR_ACCESS_DENIED ${detail}`),
            { code: 'ERR_ACCESS_DENIED', syscall },
          ));
        });
        return this;
      }
      this.connecting = true;
      this.pending = true;
      const blockedHost = host === 'localhost' && blockList?.check?.('127.0.0.1', 'ipv4')
        ? '127.0.0.1' : host;
      if (blockList?.check?.(blockedHost, String(blockedHost).includes(':') ? 'ipv6' : 'ipv4')) {
        queueMicrotask(() => {
          this.connecting = false;
          this.pending = false;
          this.emit('error', Object.assign(new Error(`Blocked address: ${blockedHost}`), { code: 'ERR_IP_BLOCKED' }));
        });
        return this;
      }
      connectOperation(this, port, host, options);
      if (this._quenchTypeOfService !== undefined && this._handle?.setTypeOfService) {
        this._handle.setTypeOfService(this._quenchTypeOfService);
      }
      if (this._handle && typeof this._handle.getAsyncId === 'function') {
        this[asyncIdSymbol] = this._handle.getAsyncId();
      }
      return this;
    }
  }
  Socket.prototype.ref = function() { 'use strict'; return this._handle?.__quenchPipeHandle ? (this._handle.ref(), this) : socketReferenceOperation(this,true); };
  Socket.prototype.unref = function() { 'use strict'; return this._handle?.__quenchPipeHandle ? (this._handle.unref(), this) : socketReferenceOperation(this,false); };
  const connect = function connect(port, host, callback) {
    const socket = new Socket(socketOptionsOperation(port, true));
    return socket.connect(port, host, callback);
  };
  function normalizeArgs(args) {
    let options;
    if (args.length === 0) {
      options = {};
    } else {
      const first = args[0];
      if (typeof first === 'object' && first !== null) {
        options = first;
      } else if (typeof first === 'string' && !(Number(first) >= 0)) {
        options = { path: first };
      } else {
        options = { port: first };
        if (args.length > 1 && typeof args[1] === 'string') options.host = args[1];
      }
    }
    const last = args[args.length - 1];
    const normalized = [options, typeof last === 'function' ? last : null];
    normalized[normalizedArgsSymbol] = true;
    return normalized;
  }
  Socket.prototype._read = readSocket;
  Socket.prototype.setNoDelay = setNoDelay;
  Socket.prototype.setKeepAlive = setKeepAlive;
  Socket.prototype.setTypeOfService = function setTypeOfService(value) {
    if (typeof value !== 'number' || Number.isNaN(value)) {
      const error = new TypeError('The "tos" argument must be of type number.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (!Number.isInteger(value) || value < 0 || value > 255) {
      const error = new RangeError('The value of "tos" is out of range. It must be an integer >= 0 && <= 255.');
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    this._quenchTypeOfService = value;
    if (this._handle?.setTypeOfService) this._handle.setTypeOfService(value);
    return this;
  };
  Socket.prototype.getTypeOfService = function getTypeOfService() {
    return this._handle?.getTypeOfService ? this._handle.getTypeOfService() : 0;
  };
  Socket.prototype.resetAndDestroy = function resetAndDestroy() {
    if (this._handle) {
      if (this.connecting) this.once('connect', () => this._reset());
      else this._reset();
    } else {
      const error = new Error('Socket is closed');
      error.code = 'ERR_SOCKET_CLOSED';
      this.destroy(error);
    }
    return this;
  };
  Socket.prototype._reset = function _reset() {
    this.resetAndClosing = true;
    return this.destroy();
  };
  function Server(options, listener) {
    if (!(this instanceof Server)) return new Server(options, listener);
    initServerEmitterOperation(this);
    this._handle = null; this._unref = false;
    this._workers = []; this._usingWorkers = false;
    if (typeof options === 'function') listener = options;
    serverOptionsOperation(options);
    if (typeof listener === 'function') this.on('connection', listener);
    initServerOptionsOperation(this, options);
  }
  Server.prototype.listen = function listen(...args) {
    this._quenchCloseScheduled = false;
    this._quenchCloseEmitted = false;
    this._quenchCloseStarted = false;
    // BoundSocket keeps its TCPWrap in a private class field, so the native
    // listen operation cannot discover the already-bound endpoint by looking
    // for `{ port }` or `{ path }`. Adopt it here through the SDK's private
    // consume hook, then let the native path register that handle as this
    // Server's listener. Node 26 gives the hook a stable symbol description.
    const first = args[0];
    if (first !== null && typeof first === 'object') {
      const prototype = Object.getPrototypeOf(first);
      const consume = prototype && Object.getOwnPropertySymbols(prototype)
        .find((symbol) => symbol.description === 'kBoundSocketConsume');
      if (consume && typeof first[consume] === 'function') {
        const pathSymbol = Object.getOwnPropertySymbols(prototype)
          .find((symbol) => symbol.description === 'kBoundSocketPath');
        const path = pathSymbol ? first[pathSymbol] : undefined;
        args[0] = first[consume]();
        if (path !== undefined) this._pipeName = path;
      }
    }
    const options = args[0];
    const signal = options && typeof options === 'object' ? options.signal : undefined;
    if (signal !== undefined) {
      if (signal === null || typeof signal !== 'object' || !('aborted' in signal) ||
          typeof signal.addEventListener !== 'function' ||
          typeof signal.removeEventListener !== 'function') {
        const received = typeof signal === 'string' ?
          ` Received type string ('${signal}')` : '';
        throw Object.assign(
          new TypeError(`The "options.signal" property must be an instance of AbortSignal.${received}`),
          { code: 'ERR_INVALID_ARG_TYPE' },
        );
      }
      const onAborted = () => this.close();
      if (signal.aborted) {
        process.nextTick(onAborted);
      } else {
        signal.addEventListener('abort', onAborted, { once: true });
        this.once('close', () => signal.removeEventListener('abort', onAborted));
      }
    }
    // The native server backend binds worker listeners locally, so Node's
    // internal net.js listenInCluster() path never reports the resulting
    // endpoint to the cluster primary. Preserve the cluster lifecycle event
    // for this direct-bind path; workers that request exclusive listeners are
    // intentionally omitted, matching Node's listenInCluster behavior.
    const clusterWorker =
      !(options && typeof options === 'object' && options.exclusive === true);
    if (clusterWorker) {
      let cluster;
      try { cluster = process.getBuiltinModule('cluster'); } catch {}
      if (cluster?.isWorker) {
        // A worker can start running its entry point after the IPC channel has
        // already disconnected. In that case the disconnect event is gone, so
        // seed the state from process.connected before listening for future
        // disconnects.
        let clusterDisconnected = process.connected === false;
        // The native direct-bind path does not register its listener in
        // internal/cluster/child.js's handle table. Cluster's disconnect
        // handshake therefore cannot close this listener itself. Close it
        // when the IPC channel disconnects so the worker can finish exiting.
        process.once('disconnect', () => {
          clusterDisconnected = true;
          if (this.listening) this.close();
        });
        this.once('listening', () => {
          // cluster.disconnect() may reach the worker before this asynchronous
          // listen finishes. Close the newly opened handle instead of leaving
          // it alive after the earlier disconnect hook already ran.
          if (clusterDisconnected) {
            this.close();
            return;
          }
          const address = this.address();
          if (!address || typeof process.send !== 'function') return;
          cluster.worker.state = 'listening';
          const isPipe = typeof address === 'string';
          process.send({
            cmd: 'NODE_CLUSTER',
            act: 'listening',
            addressType: isPipe ? -1 : address.family === 'IPv6' ? 6 : 4,
            address: isPipe ? address : address.address,
            port: isPipe ? -1 : address.port,
            fd: undefined,
          }, () => {});
        });
      }
    }
    return Reflect.apply(listenOperation, this, args);
  };
  Server.prototype.getConnections = function getConnectionsWithWorkers(callback) {
    if (!this._usingWorkers || this._workers.length === 0) {
      return getConnections.call(this, callback);
    }
    let remaining = this._workers.length;
    let total = this._connections;
    let completed = false;
    const finish = (error, count) => {
      if (completed) return;
      if (error) {
        completed = true;
        callback(error);
        return;
      }
      total += count;
      if (--remaining === 0) {
        completed = true;
        callback(null, total);
      }
    };
    for (const worker of this._workers) worker.getConnections(finish);
    return this;
  };
  Server.prototype.address = addressOperation;
  // Native close completion and worker SocketList drain can both race to
  // announce the Server's close event. Coalesce those notifications while the
  // server is closing, without changing the behavior of other event types.
  Server.prototype.emit = function emitServerEvent(event, ...args) {
    if (event === 'close' && this._quenchCloseStarted) {
      if (this._quenchCloseEmitted) return false;
      this._quenchCloseEmitted = true;
      this._quenchCloseScheduled = false;
    }
    return Reflect.apply(Object.getPrototypeOf(Server.prototype).emit,
      this, [event, ...args]);
  };
  Server.prototype.close = function closeWithWorkers(callback) {
    this._quenchCloseStarted = true;
    if (!this._usingWorkers || this._workers.length === 0) {
      const result = Reflect.apply(closeOperation, this,
        callback === undefined ? [] : [callback]);
      this._emitCloseIfDrained();
      return result;
    }
    const workers = this._workers.slice();
    // Keep the native listener close from emitting `close` before remote
    // SocketListSend instances report that their transferred sockets drained.
    this._connections++;
    const result = Reflect.apply(closeOperation, this,
      callback === undefined ? [] : [callback]);
    this._emitCloseIfDrained();
    let remaining = workers.length;
    const workerClosed = () => {
      if (--remaining !== 0) return;
      this._connections = 0;
      this._emitCloseIfDrained();
    };
    for (const worker of workers) worker.close(workerClosed);
    return result;
  };
  // Match Node's Server drain rule. Native close clears _handle immediately;
  // transferred connections keep _connections nonzero until their IPC socket
  // lists drain, at which point this schedules the one observable close event.
  Server.prototype._emitCloseIfDrained = function emitCloseIfDrained() {
    if (this._handle || this._connections || this._quenchCloseScheduled ||
        this._quenchCloseEmitted) return this;
    this._quenchCloseScheduled = true;
    process.nextTick(() => {
      this._quenchCloseScheduled = false;
      if (this._handle || this._connections || this._quenchCloseEmitted) return;
      this.emit('close');
    });
    return this;
  };
  Server.prototype[Symbol.for('nodejs.rejection')] = function(error, event, socket) {
    if (event === 'connection') socket.destroy(error);
    else this.emit('error', error);
  };
  Server.prototype._setupWorker = function(socketList) {
    this._usingWorkers = true;
    this._workers.push(socketList);
    socketList.once('exit', (socketList) => {
      const index = this._workers.indexOf(socketList);
      if (index !== -1) this._workers.splice(index, 1);
    });
  };
  Server.prototype.ref = function() { 'use strict'; return referenceOperation(this, true); };
  Server.prototype.unref = function() { 'use strict'; return referenceOperation(this, false); };
  const createServer = function createServer(options, listener) { return new Server(options, listener); };
  function Stream(options) { return new Socket(options); }
  Stream.prototype = Socket.prototype;
  Object.setPrototypeOf(Stream, Socket);
  const blockError = (code, message) => Object.assign(
    code === 'ERR_OUT_OF_RANGE' ? new RangeError(`${code}: ${message}`) : new TypeError(`${code}: ${message}`),
    { code },
  );
  const invalidArrayArgument = (name, value) => {
    let received;
    if (value === null) received = 'null';
    else if (value === undefined) received = 'undefined';
    else if (typeof value === 'string') received = `type string ('${value}')`;
    else if (typeof value === 'number' || typeof value === 'boolean') received = `type ${typeof value} (${value})`;
    else if (typeof value === 'bigint') received = `type bigint (${value}n)`;
    else if (typeof value === 'symbol') received = `type symbol (${String(value)})`;
    else received = `an instance of ${value?.constructor?.name || 'Object'}`;
    return Object.assign(
      new TypeError(`The \"${name}\" argument must be an instance of Array. Received ${received}`),
      { code: 'ERR_INVALID_ARG_TYPE' },
    );
  };
  const parseAddress = (value, family) => {
    if (value && typeof value === 'object') {
      family = value.family === 'IPv6' ? 'ipv6' : 'ipv4';
      value = value.address;
    }
    if (typeof value !== 'string') throw blockError('ERR_INVALID_ARG_TYPE', 'The "address" argument must be of type string');
    if (family !== undefined && typeof family !== 'string') throw blockError('ERR_INVALID_ARG_TYPE', 'The "type" argument must be of type string');
    family = family === undefined ? (value.includes(':') ? 'ipv6' : 'ipv4') : String(family).toLowerCase();
    if (family !== 'ipv4' && family !== 'ipv6') throw blockError('ERR_INVALID_ARG_VALUE', 'The "type" argument must be either ipv4 or ipv6');
    let text = value.toLowerCase();
    if (family === 'ipv4') {
      const parts = text.split('.');
      if (parts.length !== 4 || parts.some((part) => !/^\d{1,3}$/.test(part) || Number(part) > 255)) throw blockError('ERR_INVALID_ARG_VALUE', `Invalid IPv4 address: ${value}`);
      const n = parts.reduce((acc, part) => (acc << 8n) | BigInt(Number(part)), 0n);
      return { text, family, value: n, bits: 32 };
    }
    const zone = text.indexOf('%');
    if (zone !== -1) {
      // libuv accepts an IPv6 scope suffix but parses only an INET6_ADDRSTRLEN
      // address prefix (39 bytes) before it. Unknown and overlong scope names
      // are ignored by uv_ip6_addr, while an overlong address itself fails.
      if (zone > 39) throw blockError('ERR_INVALID_ARG_VALUE', `Invalid IPv6 address: ${value}`);
      text = text.slice(0, zone);
    }
    if (text.includes('.')) {
      const lastColon = text.lastIndexOf(':');
      const v4 = parseAddress(text.slice(lastColon + 1), 'ipv4').value;
      text = `${text.slice(0, lastColon)}:${Number((v4 >> 16n) & 65535n).toString(16)}:${Number(v4 & 65535n).toString(16)}`;
    }
    const halves = text.split('::');
    if (halves.length > 2) throw blockError('ERR_INVALID_ARG_VALUE', `Invalid IPv6 address: ${value}`);
    const left = halves[0] ? halves[0].split(':') : [];
    const right = halves.length === 2 && halves[1] ? halves[1].split(':') : [];
    const zeros = halves.length === 2 ? 8 - left.length - right.length : 0;
    const parts = [...left, ...Array(zeros).fill('0'), ...right];
    if (parts.length !== 8 || parts.some((part) => !/^[\da-f]{1,4}$/.test(part))) throw blockError('ERR_INVALID_ARG_VALUE', `Invalid IPv6 address: ${value}`);
    const n = parts.reduce((acc, part) => (acc << 16n) | BigInt(`0x${part}`), 0n);
    return { text: value, family, value: n, bits: 128 };
  };
  const addressValue = (value, family) => parseAddress(value, family);
  const ipv4Mapped = (parsed) => parsed.family === 'ipv6' && (parsed.value >> 32n) === 65535n
    ? { family: 'ipv4', value: parsed.value & 0xffffffffn, bits: 32, text: '' } : null;
  const blockListHandleKey = Symbol.for('quench.internal.blocklist.handle');
  const socketAddressHandleKey = Symbol.for('quench.internal.socketaddress.handle');
  class SocketAddress {
    constructor(options = {}) {
      if (!options || typeof options !== 'object') throw blockError('ERR_INVALID_ARG_TYPE', 'The "options" argument must be an object');
      let parsed;
      try { parsed = parseAddress(options.address, options.family); }
      catch (error) { throw blockError('ERR_INVALID_ADDRESS', error.message); }
      this.address = parsed.text;
      this.family = parsed.family === 'ipv4' ? 'IPv4' : 'IPv6';
      this.port = options.port === undefined ? 0 : options.port;
      this.flowlabel = options.flowlabel === undefined ? 0 : options.flowlabel;
      this[socketAddressHandleKey] = { address: this.address, family: this.family };
    }
    toJSON() { return { address: this.address, family: this.family, port: this.port, flowlabel: this.flowlabel }; }
    static isSocketAddress(value) { return value instanceof SocketAddress; }
  }
  class BlockList {
    constructor() {
      this._rules = [];
        this[blockListHandleKey] = {
          check: (address) => this.check(address?.address, address?.family),
          checkString: (address, family) =>
            this.check(address, family === 10 || family === 'ipv6' || family === 'IPv6' ? 'ipv6' : 'ipv4'),
        };
    }
    _insert(kind, first, last = first, prefix) {
      if (this._rules.some((rule) => rule.kind === kind && rule.family === first.family && rule.start === first.value && rule.end === last.value)) return;
      this._rules.push({ kind, family: first.family, start: first.value, end: last.value, address: first.text, endAddress: last.text, bits: first.bits, prefix });
    }
    addAddress(address, type) { const first = addressValue(address, type); this._insert('Address', first); }
    addAddresses(addresses, type) {
      if (!Array.isArray(addresses)) throw invalidArrayArgument('addresses', addresses);
      const parsed = addresses.map((address) => parseAddress(address, type));
      for (const address of parsed) this._insert('Address', address);
    }
    addRange(start, end, type) {
      const first = parseAddress(start, type); const last = parseAddress(end, type || first.family);
      if (first.family !== last.family || last.value < first.value) throw blockError('ERR_INVALID_ARG_VALUE', 'Invalid address range');
      this._insert('Range', first, last);
    }
    addSubnet(address, prefix, type) {
      const first = parseAddress(address, type);
      if (typeof prefix !== 'number') throw blockError('ERR_INVALID_ARG_TYPE', 'The "prefix" argument must be of type number');
      if (!Number.isInteger(prefix) || prefix < 0 || prefix > first.bits) throw blockError('ERR_OUT_OF_RANGE', 'The "prefix" argument is out of range');
      const shift = BigInt(first.bits - prefix); const start = (first.value >> shift) << shift;
      const end = start + ((1n << shift) - 1n);
      this._insert('Subnet', { ...first, value: start }, { ...first, value: end }, prefix);
    }
    addCIDR(cidr) {
      if (typeof cidr !== 'string') throw blockError('ERR_INVALID_ARG_TYPE', 'The "cidr" argument must be of type string');
      const slash = cidr.lastIndexOf('/'); if (slash < 0) throw blockError('ERR_INVALID_ARG_VALUE', 'Invalid CIDR');
      const address = cidr.slice(0, slash); const prefixText = cidr.slice(slash + 1);
      if (!/^\d+$/.test(prefixText)) throw blockError('ERR_INVALID_ARG_VALUE', 'Invalid CIDR');
      this.addSubnet(address, Number(prefixText));
    }
    addCIDRs(cidrs) {
      if (!Array.isArray(cidrs)) throw invalidArrayArgument('cidrs', cidrs);
      const parsed = cidrs.map((cidr) => {
        if (typeof cidr !== 'string') throw blockError('ERR_INVALID_ARG_TYPE', 'CIDR entries must be strings');
        const slash = cidr.lastIndexOf('/'); if (slash < 0 || !/^\d+$/.test(cidr.slice(slash + 1))) throw blockError('ERR_INVALID_ARG_VALUE', 'Invalid CIDR');
        return [cidr.slice(0, slash), Number(cidr.slice(slash + 1))];
      });
      for (const [address, prefix] of parsed) this.addSubnet(address, prefix);
    }
    _remove(kind, address, second, type) {
      const first = parseAddress(address, type);
      let start = first.value; let end = start;
      if (kind === 'Range') { const last = parseAddress(second, type || first.family); end = last.value; }
      if (kind === 'Subnet') {
        if (typeof second !== 'number') throw blockError('ERR_INVALID_ARG_TYPE', 'The "prefix" argument must be of type number');
        if (!Number.isInteger(second) || second < 0 || second > first.bits) throw blockError('ERR_OUT_OF_RANGE', 'The "prefix" argument is out of range');
        const shift = BigInt(first.bits - second); start = (start >> shift) << shift; end = start + ((1n << shift) - 1n);
      }
      this._rules = this._rules.filter((rule) => !(rule.kind === kind && rule.family === first.family && rule.start === start && rule.end === end));
    }
    removeAddress(address, type) { this._remove('Address', address, undefined, type); }
    removeRange(start, end, type) { this._remove('Range', start, end, type); }
    removeSubnet(address, prefix, type) { this._remove('Subnet', address, prefix, type); }
    removeCIDR(cidr) {
      if (typeof cidr !== 'string') throw blockError('ERR_INVALID_ARG_TYPE', 'The "cidr" argument must be of type string');
      const slash = cidr.lastIndexOf('/'); if (slash < 0 || !/^\d+$/.test(cidr.slice(slash + 1))) throw blockError('ERR_INVALID_ARG_VALUE', 'Invalid CIDR');
      this.removeSubnet(cidr.slice(0, slash), Number(cidr.slice(slash + 1)));
    }
    check(address, type) {
      if (type === undefined) type = address && typeof address === 'object'
        ? (address.family === 'IPv6' ? 'ipv6' : 'ipv4') : 'ipv4';
      let parsed;
      try { parsed = parseAddress(address, type); }
      catch (error) { if (error && error.code === 'ERR_INVALID_ARG_VALUE') return false; throw error; }
      const candidates = [parsed]; const mapped = ipv4Mapped(parsed); if (mapped) candidates.push(mapped);
      if (parsed.family === 'ipv4') candidates.push({ family: 'ipv6', value: (65535n << 32n) | parsed.value, bits: 128 });
      return candidates.some((candidate) => this._rules.some((rule) => rule.family === candidate.family && candidate.value >= rule.start && candidate.value <= rule.end));
    }
    clear() { this._rules = []; }
    toJSON() { return this.rules; }
    [Symbol.for('nodejs.util.inspect.custom')](depth) {
      if (depth < 0) return '[BlockList]';
      return `BlockList { rules: ${JSON.stringify(this.rules)} }`;
    }
    fromJSON(input) {
      if (typeof input === 'string') { try { input = JSON.parse(input); } catch { throw blockError('ERR_INVALID_ARG_VALUE', 'Invalid JSON'); } }
      if (!Array.isArray(input)) throw blockError('ERR_INVALID_ARG_TYPE', 'The "rules" argument must be an array');
      const parsed = [];
      for (const rule of input) {
        if (typeof rule !== 'string') throw blockError('ERR_INVALID_ARG_TYPE', 'Rules must be strings');
        const match = /^(Address|Range|Subnet): IPv(4|6) (.+)$/.exec(rule); if (!match) continue;
        try {
          const family = match[2] === '4' ? 'ipv4' : 'ipv6'; const data = match[3];
          if (match[1] === 'Address') parsed.push(['Address', data, undefined, family]);
          else if (match[1] === 'Range') { const dash = data.indexOf('-'); parsed.push(['Range', data.slice(0, dash), data.slice(dash + 1), family]); }
          else { const slash = data.lastIndexOf('/'); parsed.push(['Subnet', data.slice(0, slash), Number(data.slice(slash + 1)), family]); }
        } catch {}
      }
      for (const [kind, first, second, family] of parsed) {
        try { if (kind === 'Address') this.addAddress(first, family); else if (kind === 'Range') this.addRange(first, second, family); else this.addSubnet(first, second, family); } catch {}
      }
    }
    get rules() { return this._rules.map((rule) => `${rule.kind}: ${rule.family === 'ipv4' ? 'IPv4' : 'IPv6'} ${rule.address}${rule.kind === 'Range' ? `-${rule.endAddress}` : rule.kind === 'Subnet' ? `/${rule.prefix}` : ''}`); }
    get size() { return this._rules.length; }
  }
  BlockList.isBlockList = (value) => value instanceof BlockList;
  BlockList.PRIVATE_RANGES = Object.freeze(['10.0.0.0/8', '172.16.0.0/12', '192.168.0.0/16', '127.0.0.0/8', '169.254.0.0/16', '::1/128', 'fe80::/10', 'fc00::/7']);
  return { Socket, Stream, Server, BlockList, SocketAddress, connect, createConnection: connect, createServer, _normalizeArgs: normalizeArgs };
})"#;

/// TCP resources for the shared-VM projection. Protocol modules keep only
/// endpoint IDs and parsed state; this is the one owner of socket bytes and
/// OS handles for that execution path.
pub(crate) struct Transport {
    native_tcp_connecting: HashMap<u64, NativeConnecting>,
    native_tcp_unbound: HashMap<u64, super::tcp_owned::TcpOwned>,
    next_id: u64,
    listeners: HashMap<u64, Listener>,
    sockets: HashMap<u64, SharedSocket>,
    connecting: HashMap<u64, ConnectingSocket>,
    closing_sockets: VecDeque<(CloseTarget, ClosingSocket)>,
    pending_events: VecDeque<TransportEvent>,
}

pub(crate) enum RawStream {
    Tcp(TcpStream),
    #[cfg(unix)]
    Local(UnixStream),
}
impl RawStream {
    fn has_pending_read(&self) -> bool {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let fd = match self {
                Self::Tcp(stream) => stream.as_raw_fd(),
                #[cfg(unix)]
                Self::Local(stream) => stream.as_raw_fd(),
            };
            let mut byte = 0u8;
            // SAFETY: recv with MSG_PEEK does not mutate the socket buffer;
            // the one-byte output is valid for the duration of the call.
            let result = unsafe {
                libc::recv(
                    fd,
                    (&mut byte as *mut u8).cast(),
                    1,
                    libc::MSG_PEEK | libc::MSG_DONTWAIT,
                )
            };
            result > 0
        }
        #[cfg(not(unix))]
        {
            false
        }
    }
    fn set_nonblocking(&self, enabled: bool) -> std::io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.set_nonblocking(enabled),
            #[cfg(unix)]
            Self::Local(stream) => stream.set_nonblocking(enabled),
        }
    }
    fn shutdown(&self, direction: std::net::Shutdown) -> std::io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.shutdown(direction),
            #[cfg(unix)]
            Self::Local(stream) => stream.shutdown(direction),
        }
    }
}
impl Read for RawStream {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.read(bytes),
            #[cfg(unix)]
            Self::Local(stream) => stream.read(bytes),
        }
    }
}
impl Write for RawStream {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.write(bytes),
            #[cfg(unix)]
            Self::Local(stream) => stream.write(bytes),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.flush(),
            #[cfg(unix)]
            Self::Local(stream) => stream.flush(),
        }
    }
}

enum SocketStream {
    Plain(RawStream),
    Encrypted(encrypted::EncryptedStream),
}
impl SocketStream {
    fn raw(&self) -> &RawStream {
        match self {
            Self::Plain(raw) => raw,
            Self::Encrypted(tls) => tls.raw(),
        }
    }
    fn set_nonblocking(&self, enabled: bool) -> std::io::Result<()> {
        self.raw().set_nonblocking(enabled)
    }
    fn has_pending_read(&self) -> bool {
        self.raw().has_pending_read()
    }
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(raw) => raw.read(bytes),
            Self::Encrypted(tls) => tls.read(bytes),
        }
    }
    fn write(&mut self, bytes: &[u8], offset: usize) -> std::io::Result<usize> {
        match self {
            Self::Plain(raw) => raw.write(&bytes[offset..]),
            Self::Encrypted(tls) => tls.write(bytes, offset),
        }
    }
    fn write_raw(&mut self, bytes: &[u8], offset: usize) -> std::io::Result<usize> {
        match self {
            Self::Plain(raw) => raw.write(&bytes[offset..]),
            Self::Encrypted(tls) => tls.write_raw(&bytes[offset..]),
        }
    }
    fn write_pending(&mut self, bytes: &[u8], offset: usize, raw: bool) -> std::io::Result<usize> {
        if raw {
            self.write_raw(bytes, offset)
        } else {
            self.write(bytes, offset)
        }
    }
    fn shutdown_write(&mut self) -> std::io::Result<bool> {
        match self {
            Self::Plain(raw) => {
                raw.shutdown(std::net::Shutdown::Write)?;
                Ok(true)
            }
            Self::Encrypted(tls) => tls.shutdown_write(),
        }
    }
    fn shutdown_write_raw(&self) -> std::io::Result<()> {
        self.raw().shutdown(std::net::Shutdown::Write)
    }
}

enum ListenerSocket {
    Tcp(TcpListener),
    #[cfg(unix)]
    Local {
        listener: UnixListener,
        path: Option<std::path::PathBuf>,
        display: String,
    },
}
impl ListenerSocket {
    fn accept(&self) -> std::io::Result<SocketStream> {
        match self {
            Self::Tcp(listener) => listener
                .accept()
                .map(|(stream, _)| SocketStream::Plain(RawStream::Tcp(stream))),
            #[cfg(unix)]
            Self::Local { listener, .. } => listener
                .accept()
                .map(|(stream, _)| SocketStream::Plain(RawStream::Local(stream))),
        }
    }
}
impl Drop for ListenerSocket {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Self::Local {
            path: Some(path), ..
        } = self
        {
            let _ = std::fs::remove_file(path);
        }
    }
}

// The reference bit belongs to the physical listener, independently of the
// public Server object and accepted sockets. Closing retains handle queries
// through callback dispatch, then releases them on the next native poll.
struct Listener {
    state: ListenerState,
    referenced: bool,
}
enum ListenerState {
    Open(ListenerSocket),
    CloseRequested,
    Closing,
}
impl Listener {
    fn new(socket: ListenerSocket) -> Self {
        Self {
            state: ListenerState::Open(socket),
            referenced: true,
        }
    }
    fn socket(&self) -> Option<&ListenerSocket> {
        match &self.state {
            ListenerState::Open(socket) => Some(socket),
            _ => None,
        }
    }
}

enum ConnectFailure {
    Tcp(TcpConnectFailure),
    #[cfg(unix)]
    Local {
        error: std::io::Error,
        path: String,
    },
}

// The reference bit transfers with the socket between disjoint lifecycle maps.
struct ConnectingSocket {
    completion: ConnectCompletion,
    referenced: bool,
    protocol: SocketProtocol,
    options: TcpOptions,
    read_demand: ReadDemand,
    pending_write: Vec<u8>,
}
#[derive(Clone, Copy)]
enum SocketProtocol {
    Tcp,
    Local,
}
#[derive(Default)]
struct TcpOptions {
    no_delay: Option<bool>,
    keep_alive: Option<TcpKeepAlive>,
    type_of_service: Option<u8>,
}
#[derive(Clone, Copy)]
pub(crate) struct TcpKeepAlive {
    pub(crate) enabled: bool,
    pub(crate) delay: u32,
    pub(crate) interval: u32,
    pub(crate) count: u32,
}
enum ConnectCompletion {
    ExternalLookup,
    NativeRequest(Receiver<Result<SocketStream, ConnectFailure>>),
}
struct ClosingSocket {
    referenced: bool,
    phase: SocketClosePhase,
}
enum SocketClosePhase {
    Requested,
    Closing,
}

enum DeliveryPhase {
    Pending,
    DeliveringConnection,
    Delivered,
    ReadPending,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReadDemand {
    Idle,
    Active,
}

struct SharedSocket {
    stream: SocketStream,
    read_demand: ReadDemand,
    delivery: DeliveryPhase,
    referenced: bool,
    pending_write: Vec<u8>,
    write_failure: Option<std::io::Error>,
    write_offset: usize,
    completions: VecDeque<WriteBoundary>,
    end_after_write: bool,
    raw_write: bool,
    raw_end_after_write: bool,
    read_ended: bool,
    write_ended: bool,
    shutdown_request: Option<RootId>,
}

impl SharedSocket {
    fn new(stream: SocketStream, read_demand: ReadDemand, referenced: bool) -> Self {
        Self {
            stream,
            read_demand,
            delivery: DeliveryPhase::Pending,
            referenced,
            pending_write: Vec::new(),
            write_failure: None,
            write_offset: 0,
            completions: VecDeque::new(),
            end_after_write: false,
            raw_write: false,
            raw_end_after_write: false,
            read_ended: false,
            write_ended: false,
            shutdown_request: None,
        }
    }
}

struct WriteBoundary {
    end: usize,
    owner: WriteOwner,
}
enum WriteOwner {
    Guest,
    Native(RootId),
}

pub(crate) enum TransportEvent {
    Accepted {
        listener: u64,
        socket: u64,
    },
    Connected {
        socket: u64,
    },
    Secure {
        socket: u64,
    },
    WriteComplete {
        socket: u64,
    },
    NativeWriteComplete {
        request: RootId,
    },
    NativeConnectComplete {
        request: RootId,
        status: i32,
    },
    NativeShutdownComplete {
        request: RootId,
    },
    ConnectError {
        socket: u64,
        failure: TcpConnectFailure,
    },
    #[cfg(unix)]
    LocalConnectError {
        socket: u64,
        error: std::io::Error,
        path: String,
    },
    Data {
        socket: u64,
        bytes: Vec<u8>,
    },
    End {
        socket: u64,
    },
    DatagramCloseCompleted {
        id: u64,
    },
    CloseCompleted {
        socket: u64,
    },
    Error {
        socket: u64,
        error: std::io::Error,
        syscall: &'static str,
    },
}

impl Transport {
    pub(crate) fn new() -> Self {
        Self {
            native_tcp_connecting: HashMap::new(),
            native_tcp_unbound: HashMap::new(),
            next_id: FIRST_TRANSPORT_ID,
            listeners: HashMap::new(),
            sockets: HashMap::new(),
            connecting: HashMap::new(),
            closing_sockets: VecDeque::new(),
            pending_events: VecDeque::new(),
        }
    }

    fn id(&mut self) -> Result<u64, String> {
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or_else(|| "shared TCP resource identifier space exhausted".to_owned())?;
        Ok(id)
    }
}

impl Default for Transport {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn listen_with_backlog(
    transport: &mut Transport,
    address: SocketAddr,
    backlog: i32,
) -> Result<u64, std::io::Error> {
    listen_with_reuse_port(transport, address, false, backlog)
}

pub(super) fn listen_with_reuse_port(
    transport: &mut Transport,
    address: SocketAddr,
    reuse_port: bool,
    backlog: i32,
) -> Result<u64, std::io::Error> {
    let listener = bind_listener(address, reuse_port, backlog)?;
    listener.set_nonblocking(true)?;
    let id = transport.id().map_err(std::io::Error::other)?;
    transport
        .listeners
        .insert(id, Listener::new(ListenerSocket::Tcp(listener)));
    Ok(id)
}

// Node's net.Server.listen uses 511 when no backlog was supplied.
pub(super) const DEFAULT_TCP_LISTEN_BACKLOG: i32 = 511;

pub(crate) fn listen_backlog(
    context: &mut NativeContext<'_, NodeHost>,
    options: Option<RootId>,
) -> Result<i32, RootedError> {
    let Some(options) = options else {
        return Ok(DEFAULT_TCP_LISTEN_BACKLOG);
    };
    let key = context.string_rooted("backlog");
    let value = context.get_property_rooted(options, key)?;
    if !context.truthy_rooted(value)? {
        return Ok(DEFAULT_TCP_LISTEN_BACKLOG);
    }
    convert_listen_backlog(context, value)
}

pub(super) fn convert_listen_backlog(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<i32, RootedError> {
    // TCPWrap::Listen uses ToInt32 after the JavaScript truthy default.
    // Apply ToNumber first so BigInt and Symbol preserve that conversion's error.
    let number = context.to_number(value)?;
    let number = context.number(number);
    let mask = context.number(super::net_socket_options::SIGNED_INTEGER_MASK);
    let bits =
        context.arithmetic_rooted(quench_runtime::NativeArithmetic::BitwiseAnd, number, mask)?;
    Ok(context.to_number(bits)? as i32)
}

fn bind_listener(
    address: SocketAddr,
    reuse_port: bool,
    backlog: i32,
) -> std::io::Result<TcpListener> {
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "aix"
    )))]
    if reuse_port {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "TCP reusePort is unsupported on this platform",
        ));
    }
    let socket = socket2::Socket::new(
        socket2::Domain::for_address(address),
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let fd = socket.as_raw_fd();
        // Node listeners are close-on-exec. `process.execve()` must release a
        // listening port when the process image is replaced.
        let descriptor_flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        if descriptor_flags < 0
            || unsafe { libc::fcntl(fd, libc::F_SETFD, descriptor_flags | libc::FD_CLOEXEC) } < 0
        {
            return Err(std::io::Error::last_os_error());
        }
    }
    // libuv enables address reuse for Unix TCP listeners and keeps
    // SO_REUSEADDR disabled for Windows TCP binds.
    #[cfg(unix)]
    socket.set_reuse_address(true)?;
    if reuse_port {
        #[cfg(any(
            target_os = "linux",
            target_os = "android",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "aix"
        ))]
        {
            #[cfg(target_os = "freebsd")]
            socket.set_reuse_port_lb(true)?;
            #[cfg(not(target_os = "freebsd"))]
            socket.set_reuse_port(true)?;
        }
    }
    socket.bind(&address.into())?;
    socket.listen(backlog)?;
    Ok(socket.into())
}

pub(crate) fn connect(transport: &mut Transport, host: String, port: u16) -> Result<u64, String> {
    let id = transport.id()?;
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name(format!("quench-node-tcp-{id}"))
        .spawn(move || {
            let result = connect_with_timeout(&host, port)
                .map(|stream| SocketStream::Plain(RawStream::Tcp(stream)))
                .and_then(|stream| {
                    stream
                        .set_nonblocking(true)
                        .map(|()| stream)
                        .map_err(TcpConnectFailure::Setup)
                });
            let _ = sender.send(result.map_err(ConnectFailure::Tcp));
        })
        .map_err(|error| format!("cannot start TCP connect: {error}"))?;
    transport.connecting.insert(
        id,
        ConnectingSocket {
            completion: ConnectCompletion::NativeRequest(receiver),
            referenced: true,
            protocol: SocketProtocol::Tcp,
            options: TcpOptions::default(),
            read_demand: ReadDemand::Active,
            pending_write: Vec::new(),
        },
    );
    Ok(id)
}

pub(super) fn reserve_lookup(transport: &mut Transport) -> Result<u64, String> {
    let id = transport.id()?;
    transport.connecting.insert(
        id,
        ConnectingSocket {
            completion: ConnectCompletion::ExternalLookup,
            referenced: true,
            protocol: SocketProtocol::Tcp,
            options: TcpOptions::default(),
            read_demand: ReadDemand::Active,
            pending_write: Vec::new(),
        },
    );
    Ok(id)
}

pub(super) fn is_looking_up(transport: &Transport, id: u64) -> bool {
    transport
        .connecting
        .get(&id)
        .is_some_and(|socket| matches!(socket.completion, ConnectCompletion::ExternalLookup))
}

pub(super) fn connect_resolved(
    transport: &mut Transport,
    id: u64,
    addresses: Vec<String>,
    port: u16,
    local_address: Option<IpAddr>,
    local_port: u16,
) -> Result<(), String> {
    if !is_looking_up(transport, id) {
        return Ok(());
    }
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name(format!("quench-node-tcp-{id}"))
        .spawn(move || {
            let mut result = Err(TcpConnectFailure::EmptyDestinations);
            for address in addresses {
                result = connect_resolved_address(&address, port, local_address, local_port);
                if result.is_ok() {
                    break;
                }
            }
            let result = result
                .map(|stream| SocketStream::Plain(RawStream::Tcp(stream)))
                .and_then(|stream| {
                    stream
                        .set_nonblocking(true)
                        .map(|()| stream)
                        .map_err(TcpConnectFailure::Setup)
                })
                .map_err(ConnectFailure::Tcp);
            let _ = sender.send(result);
        })
        .map_err(|error| format!("cannot start TCP connect: {error}"))?;
    transport
        .connecting
        .get_mut(&id)
        .expect("checked lookup owner")
        .completion = ConnectCompletion::NativeRequest(receiver);
    Ok(())
}

pub(crate) fn address(transport: &Transport, listener: u64) -> Option<SocketAddr> {
    transport
        .listeners
        .get(&listener)
        .and_then(Listener::socket)
        .and_then(|listener| match listener {
            ListenerSocket::Tcp(listener) => listener.local_addr().ok(),
            #[cfg(unix)]
            ListenerSocket::Local { .. } => None,
        })
}

pub(crate) fn socket_address(
    transport: &Transport,
    id: u64,
    peer: bool,
) -> std::io::Result<SocketAddr> {
    match transport.sockets.get(&id).map(|socket| socket.stream.raw()) {
        Some(RawStream::Tcp(stream)) => {
            if peer {
                stream.peer_addr()
            } else {
                stream.local_addr()
            }
        }
        _ => Err(std::io::Error::from_raw_os_error(libc::EBADF)),
    }
}

pub(crate) fn write(transport: &mut Transport, socket: u64, bytes: &[u8]) -> Result<(), String> {
    if let Some(stream) = transport.sockets.get_mut(&socket) {
        stream.pending_write.extend_from_slice(bytes);
        return Ok(());
    }
    if let Some(connecting) = transport.connecting.get_mut(&socket) {
        // Node accepts writes as soon as a socket is assigned, even while the
        // native connection is still pending. Flush them once connect completes.
        connecting.pending_write.extend_from_slice(bytes);
        return Ok(());
    }
    Err("shared TCP socket is not connected".to_owned())
}

/// Attempt a socket write synchronously before falling back to the normal
/// event-loop queue. Node's `net.Socket.write()` reaches the kernel in the
/// caller's turn when possible; this matters for callers that immediately
/// inspect pending receive data on the peer without allowing an I/O poll.
pub(crate) fn write_immediate(
    transport: &mut Transport,
    socket: u64,
    bytes: &[u8],
) -> Result<(), String> {
    let stream = transport
        .sockets
        .get_mut(&socket)
        .ok_or_else(|| "shared TCP socket is not connected".to_owned())?;
    if bytes.is_empty() {
        return Ok(());
    }
    if stream.write_offset < stream.pending_write.len() {
        stream.pending_write.extend_from_slice(bytes);
        return Ok(());
    }
    if !stream.pending_write.is_empty() {
        stream.pending_write.clear();
        stream.write_offset = 0;
        stream.raw_write = false;
    }
    match stream.stream.write(bytes, 0) {
        Ok(written) if written == bytes.len() => Ok(()),
        Ok(0) => Err(std::io::Error::from(std::io::ErrorKind::WriteZero).to_string()),
        Ok(written) => {
            stream.pending_write.extend_from_slice(bytes);
            stream.write_offset = written;
            stream.raw_write = false;
            Ok(())
        }
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
            ) =>
        {
            stream.pending_write.extend_from_slice(bytes);
            stream.write_offset = 0;
            stream.raw_write = false;
            Ok(())
        }
        Err(error) => Err(error.to_string()),
    }
}

/// Record the byte boundary for a guest stream write. Raw protocol writes
/// share the transport without creating guest completion callbacks.
pub(crate) fn write_with_completion(
    transport: &mut Transport,
    socket: u64,
    bytes: &[u8],
    raw: bool,
) -> Result<(), String> {
    write(transport, socket, bytes)?;
    let stream = transport
        .sockets
        .get_mut(&socket)
        .expect("write checked live socket");
    stream.raw_write = raw;
    stream.completions.push_back(WriteBoundary {
        end: stream.pending_write.len(),
        owner: WriteOwner::Guest,
    });
    // Match uv_try_write admission for plain native streams. Keep callback
    // boundaries until poll, including completely synchronous writes.
    if stream.write_failure.is_none()
        && (raw || matches!(&stream.stream, SocketStream::Plain(_)))
        && stream.write_offset < stream.pending_write.len()
    {
        match stream
            .stream
            .write_pending(&stream.pending_write, stream.write_offset, raw)
        {
            Ok(0) => stream.write_failure = Some(std::io::ErrorKind::WriteZero.into()),
            Ok(written) => stream.write_offset += written,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(error) => stream.write_failure = Some(error),
        }
    }
    Ok(())
}

/// A TLS wrapper and its supplied net.Socket share a transport ID. The
/// current net socket root tracks the active wrapper; an older receiver is
/// the original raw socket and must keep raw TCP write/end semantics.
fn is_raw_wrapped_socket(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
    id: u64,
) -> Result<bool, RootedError> {
    let state = context.host_mut().shared_state();
    let current = state
        .borrow()
        .net_sockets
        .get(&id)
        .map(|socket| socket.root);
    match current {
        Some(current) => Ok(!context.same_value_rooted(socket, current)?),
        None => Ok(false),
    }
}

/// Application bytes accepted by the native transport but not yet written.
/// Derive this from the write buffer: advancing/clearing it is the only owner
/// of transport progress. Connecting and closed sockets have no native queue.
pub(crate) fn socket_write_queue_size(transport: &Transport, id: u64) -> usize {
    transport
        .sockets
        .get(&id)
        .map_or(0, |socket| socket.pending_write.len() - socket.write_offset)
}

pub(crate) fn socket_is_open(transport: &Transport, id: u64) -> bool {
    transport.sockets.contains_key(&id) || transport.connecting.contains_key(&id)
}

pub(crate) fn is_connected(transport: &Transport, socket: u64) -> bool {
    transport.sockets.contains_key(&socket)
}

pub(crate) fn end(transport: &mut Transport, socket: u64, raw: bool) -> Result<(), String> {
    if let Some(stream) = transport.sockets.get_mut(&socket) {
        stream.end_after_write = true;
        stream.raw_end_after_write = raw;
    }
    Ok(())
}

pub(crate) fn close_listener(transport: &mut Transport, listener: u64) {
    if let Some(listener) = transport.listeners.get_mut(&listener) {
        if matches!(listener.state, ListenerState::Open(_)) {
            listener.state = ListenerState::CloseRequested;
        }
    }
}

pub(crate) fn close_socket(transport: &mut Transport, id: u64) {
    let referenced = transport
        .connecting
        .remove(&id)
        .map(|socket| socket.referenced)
        .or_else(|| {
            transport
                .sockets
                .remove(&id)
                .map(|socket| socket.referenced)
        });
    if let Some(referenced) = referenced {
        request_socket_close(transport, id, referenced);
    }
}

pub(crate) fn reset_socket(transport: &Transport, id: u64) -> std::io::Result<()> {
    let socket = transport
        .sockets
        .get(&id)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::ENOTCONN))?;
    match socket.stream.raw() {
        RawStream::Tcp(stream) => {
            socket2::SockRef::from(stream).set_linger(Some(std::time::Duration::ZERO))
        }
        #[cfg(unix)]
        RawStream::Local(_) => Err(std::io::Error::from_raw_os_error(libc::EOPNOTSUPP)),
    }
}

pub(crate) fn poll(transport: &mut Transport) -> Vec<TransportEvent> {
    transport
        .listeners
        .retain(|_, listener| match listener.state {
            ListenerState::Closing => false,
            ListenerState::CloseRequested => {
                listener.state = ListenerState::Closing;
                true
            }
            ListenerState::Open(_) => true,
        });
    let mut events = transport.pending_events.drain(..).collect::<Vec<_>>();
    transport
        .closing_sockets
        .retain_mut(|(id, socket)| match socket.phase {
            SocketClosePhase::Closing => {
                events.push(match *id {
                    CloseTarget::Stream(socket) => TransportEvent::CloseCompleted { socket },
                    CloseTarget::Datagram(id) => TransportEvent::DatagramCloseCompleted { id },
                });
                false
            }
            SocketClosePhase::Requested => {
                socket.phase = SocketClosePhase::Closing;
                true
            }
        });
    events.extend(poll_connects(transport));
    events.extend(poll_native_connects(transport));
    events.extend(poll_accepts(transport));
    events.extend(poll_sockets(transport));
    events
}

pub(crate) fn queue_http_data(transport: &mut Transport, socket: u64) {
    transport.pending_events.push_back(TransportEvent::Data {
        socket,
        bytes: Vec::new(),
    });
}

pub(crate) fn has_work(transport: &Transport) -> bool {
    !transport.native_tcp_connecting.is_empty()
        || transport
            .listeners
            .values()
            .any(|listener| match listener.state {
                ListenerState::Open(_) => listener.referenced,
                ListenerState::CloseRequested | ListenerState::Closing => true,
            })
        || transport.sockets.values().any(|socket| {
            (socket.referenced && !socket.read_ended && socket.read_demand == ReadDemand::Active)
                || socket.write_offset < socket.pending_write.len()
                || !socket.completions.is_empty()
                || (socket.end_after_write && !socket.write_ended)
        })
        || !transport.closing_sockets.is_empty()
        || transport
            .connecting
            .values()
            .any(|socket| matches!(socket.completion, ConnectCompletion::NativeRequest(_)))
}

pub(crate) fn cleanup(transport: &mut Transport) {
    transport.listeners.clear();
    transport.native_tcp_connecting.clear();
    transport.native_tcp_unbound.clear();
    transport.sockets.clear();
    transport.connecting.clear();
    transport.closing_sockets.clear();
}

pub(crate) const fn poll_interval() -> std::time::Duration {
    std::time::Duration::from_millis(1)
}

fn poll_connects(transport: &mut Transport) -> Vec<TransportEvent> {
    let mut events = Vec::new();
    let ids = transport.connecting.keys().copied().collect::<Vec<_>>();
    for id in ids {
        let Some(connection) = transport.connecting.get(&id) else {
            continue;
        };
        let ConnectCompletion::NativeRequest(receiver) = &connection.completion else {
            continue;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => {
                continue;
            }
            Err(TryRecvError::Disconnected) => {
                Err(ConnectFailure::Tcp(TcpConnectFailure::WorkerStopped))
            }
        };
        let connection = transport
            .connecting
            .remove(&id)
            .expect("observed connecting socket");
        let referenced = connection.referenced;
        match result {
            Ok(stream) => {
                if let RawStream::Tcp(tcp) = stream.raw() {
                    if let Some(enabled) = connection.options.no_delay {
                        let _ = tcp.set_nodelay(enabled);
                    }
                    if let Some(options) = connection.options.keep_alive {
                        let _ = apply_tcp_keep_alive(tcp, options);
                    }
                    if let Some(tos) = connection.options.type_of_service {
                        let _ = apply_tcp_type_of_service(tcp, tos);
                    }
                }
                let mut socket = SharedSocket::new(stream, connection.read_demand, referenced);
                socket.pending_write = connection.pending_write;
                transport.sockets.insert(id, socket);
                events.push(TransportEvent::Connected { socket: id });
            }
            Err(ConnectFailure::Tcp(failure)) => {
                request_socket_close(transport, id, referenced);
                events.push(TransportEvent::ConnectError {
                    socket: id,
                    failure,
                });
            }
            #[cfg(unix)]
            Err(ConnectFailure::Local { error, path }) => {
                request_socket_close(transport, id, referenced);
                events.push(TransportEvent::LocalConnectError {
                    socket: id,
                    error,
                    path,
                });
            }
        }
    }
    events
}

fn poll_accepts(transport: &mut Transport) -> Vec<TransportEvent> {
    let listeners = transport
        .listeners
        .iter_mut()
        .flat_map(|(listener_id, listener)| {
            let mut accepted = Vec::new();
            let Some(listener) = listener.socket() else {
                return accepted;
            };
            loop {
                match listener.accept() {
                    Ok(stream) => accepted.push((*listener_id, stream)),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => break,
                }
            }
            accepted
        })
        .collect::<Vec<_>>();
    let mut events = Vec::with_capacity(listeners.len());
    for (listener, stream) in listeners {
        if stream.set_nonblocking(true).is_err() {
            continue;
        }
        let Ok(socket) = transport.id() else {
            continue;
        };
        transport
            .sockets
            .insert(socket, SharedSocket::new(stream, ReadDemand::Active, true));
        events.push(TransportEvent::Accepted { listener, socket });
    }
    events
}

fn poll_sockets(transport: &mut Transport) -> Vec<TransportEvent> {
    let mut events = Vec::new();
    let mut terminal = Vec::new();
    let ids = transport.sockets.keys().copied().collect::<Vec<_>>();
    let mut buffer = [0; TCP_READ_CHUNK];
    for id in ids {
        let Some(socket) = transport.sockets.get_mut(&id) else {
            continue;
        };
        if !matches!(socket.delivery, DeliveryPhase::Delivered) {
            continue;
        }
        if let SocketStream::Encrypted(tls) = &mut socket.stream {
            match tls.handshake() {
                Ok(true) => events.push(TransportEvent::Secure { socket: id }),
                Ok(false) => {}
                Err(error) => {
                    events.push(TransportEvent::Error {
                        socket: id,
                        error,
                        syscall: "read",
                    });
                    terminal.push(id);
                    continue;
                }
            }
            if !tls.ready() {
                continue;
            }
        }
        // Deliver completed requests before attempting a later failing write.
        poll_write_completions(socket, id, &mut events);
        if let Some(error) = socket.write_failure.take() {
            events.push(TransportEvent::Error {
                socket: id,
                error,
                syscall: "write",
            });
            terminal.push(id);
            continue;
        }
        if socket.write_offset < socket.pending_write.len() {
            match socket.stream.write_pending(
                &socket.pending_write,
                socket.write_offset,
                socket.raw_write,
            ) {
                Ok(0) => {
                    events.push(TransportEvent::Error {
                        socket: id,
                        error: std::io::Error::from(std::io::ErrorKind::WriteZero),
                        syscall: "write",
                    });
                    terminal.push(id);
                    continue;
                }
                Ok(written) => socket.write_offset += written,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => {
                    events.push(TransportEvent::Error {
                        socket: id,
                        error,
                        syscall: "write",
                    });
                    terminal.push(id);
                    continue;
                }
            }
        }
        poll_write_completions(socket, id, &mut events);
        if socket.write_offset == socket.pending_write.len() {
            socket.pending_write.clear();
            socket.write_offset = 0;
            socket.raw_write = false;
        }
        if socket.end_after_write && socket.pending_write.is_empty() {
            let shutdown = if socket.raw_end_after_write {
                socket.stream.shutdown_write_raw().map(|()| true)
            } else {
                socket.stream.shutdown_write()
            };
            match shutdown {
                Ok(true) => {
                    socket.end_after_write = false;
                    socket.raw_end_after_write = false;
                    socket.write_ended = true;
                    if let Some(request) = socket.shutdown_request.take() {
                        events.push(TransportEvent::NativeShutdownComplete { request });
                    }
                }
                Ok(false) => {}
                Err(error)
                    if socket.raw_end_after_write
                        && error.raw_os_error() == Some(libc::ENOTCONN) =>
                {
                    // A caller may half-close the original raw socket while
                    // its TLS wrapper still owns the read side. If the peer
                    // closes immediately after receiving the final raw record,
                    // shutdown(Write) can report ENOTCONN even though the peer
                    // has already queued a TLS alert for us to read. Preserve
                    // the receive half and let the pending alert reach TLS.
                    socket.end_after_write = false;
                    socket.raw_end_after_write = false;
                    socket.write_ended = true;
                    if let Some(request) = socket.shutdown_request.take() {
                        events.push(TransportEvent::NativeShutdownComplete { request });
                    }
                }
                Err(error) => {
                    events.push(TransportEvent::Error {
                        socket: id,
                        error,
                        syscall: "write",
                    });
                    terminal.push(id);
                    continue;
                }
            }
        }
        // Writes and TLS handshakes have independent progress; application
        // bytes are consumed only while the native read request is active.
        if socket.read_demand == ReadDemand::Idle {
            continue;
        }
        let mut received = Vec::new();
        let mut ended = false;
        let mut failure = None;
        while !socket.read_ended {
            match socket.stream.read(&mut buffer) {
                Ok(0) => {
                    ended = true;
                    socket.read_ended = true;
                    break;
                }
                Ok(count) => {
                    received.extend_from_slice(&buffer[..count]);
                    socket.delivery = DeliveryPhase::ReadPending;
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        if !received.is_empty() {
            events.push(TransportEvent::Data {
                socket: id,
                bytes: received,
            });
        }
        if let Some(error) = failure {
            events.push(TransportEvent::Error {
                socket: id,
                error,
                syscall: "read",
            });
            terminal.push(id);
        } else if ended {
            events.push(TransportEvent::End { socket: id });
        }
        // Guest Duplex EOF/finish owns destruction after queued bytes drain.
        // Closing here can destroy a readable before its queued data/end jobs.
        // HTTP endpoints close explicitly in their EOF projection.
    }
    for id in terminal {
        close_socket(transport, id);
    }
    events
}

fn connect_with_timeout(host: &str, port: u16) -> Result<TcpStream, TcpConnectFailure> {
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|error| TcpConnectFailure::Resolve {
            error,
            hostname: host.to_owned(),
        })?;
    let mut last_error = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, TCP_CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = Some(TcpConnectFailure::Connect { error, address }),
        }
    }
    Err(last_error.unwrap_or_else(|| TcpConnectFailure::Resolve {
        error: std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "resolver returned no addresses",
        ),
        hostname: host.to_owned(),
    }))
}

fn connect_resolved_address(
    host: &str,
    port: u16,
    local_address: Option<IpAddr>,
    local_port: u16,
) -> Result<TcpStream, TcpConnectFailure> {
    let address = host
        .parse::<IpAddr>()
        .map(|ip| SocketAddr::new(ip, port))
        .map_err(|error| TcpConnectFailure::Connect {
            error: std::io::Error::new(std::io::ErrorKind::InvalidInput, error),
            address: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port),
        })?;
    if local_address.is_none() && local_port == 0 {
        return TcpStream::connect_timeout(&address, TCP_CONNECT_TIMEOUT)
            .map_err(|error| TcpConnectFailure::Connect { error, address });
    }

    let source_ip = local_address.unwrap_or_else(|| match address.ip() {
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    });
    if source_ip.is_ipv4() != address.is_ipv4() {
        return Err(TcpConnectFailure::Connect {
            error: std::io::Error::from(std::io::ErrorKind::AddrNotAvailable),
            address,
        });
    }
    let socket = socket2::Socket::new(
        socket2::Domain::for_address(address),
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )
    .map_err(|error| TcpConnectFailure::Connect { error, address })?;
    socket
        .bind(&SocketAddr::new(source_ip, local_port).into())
        .map_err(|error| TcpConnectFailure::Connect { error, address })?;
    socket
        .connect_timeout(&address.into(), TCP_CONNECT_TIMEOUT)
        .map_err(|error| TcpConnectFailure::Connect { error, address })?;
    Ok(socket.into())
}

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    let initialize = context.host_function_with_data(
        crate::host::shared_vm::operation("netBoundSocketProjection"),
        module,
    )?;
    context.define_lazy_data_properties_rooted(
        module,
        &["BoundSocket", "_TransferredBoundSocket"],
        true,
        initialize,
    )?;

    for (name, operation) in [
        ("isIP", "netIsIP"),
        ("isIPv4", "netIsIPv4"),
        ("isIPv6", "netIsIPv6"),
    ] {
        let function = context.host_function(crate::host::shared_vm::operation(operation))?;
        let key = context.string_rooted(name);
        if !context.set_property_rooted(module, key, function, module)? {
            return Err(RootedError::host(format!("cannot install net.{name}")));
        }
    }
    for (name, operation) in [
        ("getDefaultAutoSelectFamily", "netGetAutoSelectFamily"),
        ("setDefaultAutoSelectFamily", "netSetAutoSelectFamily"),
        (
            "getDefaultAutoSelectFamilyAttemptTimeout",
            "netGetAutoSelectFamilyAttemptTimeout",
        ),
        (
            "setDefaultAutoSelectFamilyAttemptTimeout",
            "netSetAutoSelectFamilyAttemptTimeout",
        ),
    ] {
        let function = context.host_function(crate::host::shared_vm::operation(operation))?;
        let key = context.string_rooted(name);
        if !context.set_property_rooted(module, key, function, module)? {
            return Err(RootedError::host("cannot install shared net binding"));
        }
    }
    // Ordinary function facades preserve Node's public constructibility and metadata.
    let callable_names = [
        "isIP",
        "isIPv4",
        "isIPv6",
        "getDefaultAutoSelectFamily",
        "setDefaultAutoSelectFamily",
        "getDefaultAutoSelectFamilyAttemptTimeout",
        "setDefaultAutoSelectFamilyAttemptTimeout",
    ];
    let mut callables = Vec::new();
    for name in callable_names {
        let key = context.string_rooted(name);
        callables.push(context.get_property_rooted(module, key)?);
    }
    let facade_factory =
        context.evaluate_script_rooted(CALLABLE_FACADE, "node:net/callable-facade.js")?;
    let undefined = context.undefined();
    let facade = context.call_rooted(facade_factory, undefined, &callables)?;
    for name in callable_names {
        let key = context.string_rooted(name);
        let function = context.get_property_rooted(facade, key)?;
        context.set_property_rooted(module, key, function, module)?;
    }
    let connect = context.host_function(crate::host::shared_vm::operation("netConnect"))?;
    let write = context.host_function(crate::host::shared_vm::operation("netSocketWrite"))?;
    let end = context.host_function(crate::host::shared_vm::operation("netSocketEnd"))?;
    // Destruction latches its outcome before guest error handlers run.
    let close_outcome = context.symbol_rooted(Some("kSocketCloseHadError"));
    let destroy = context.host_function_with_data(
        crate::host::shared_vm::operation("netSocketDestroy"),
        close_outcome,
    )?;
    let close_completed = context.host_function_with_data(
        crate::host::shared_vm::operation("netSocketCloseCompleted"),
        close_outcome,
    )?;
    let close_completed = context.retain(close_completed)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .net_socket_close_completed = Some(close_completed);
    let encoding =
        context.host_function(crate::host::shared_vm::operation("netSocketSetEncoding"))?;
    let listen =
        context.host_constructor_named(crate::host::shared_vm::operation("netServerListen"), "")?;
    let close =
        context.host_constructor_named(crate::host::shared_vm::operation("netServerClose"), "")?;
    let address = context
        .host_constructor_named(crate::host::shared_vm::operation("netServerAddress"), "")?;
    let stream = crate::host::shared_vm::commonjs::stream_module(context)?;
    let duplex_key = context.string_rooted("Duplex");
    let duplex = context.get_property_rooted(stream, duplex_key)?;
    let buffer_module = crate::host::shared_vm::commonjs::buffer_module(context)?;
    let key = context.string_rooted("Buffer");
    let buffer = context.get_property_rooted(buffer_module, key)?;
    let key = context.string_rooted("byteLength");
    let byte_length = context.get_property_rooted(buffer, key)?;
    let account = context.host_function_with_data(
        crate::host::shared_vm::operation("netSocketAccountWrite"),
        byte_length,
    )?;
    let endpoint =
        context.host_function(crate::host::shared_vm::operation("netNormalizeEndpoint"))?;
    let reference =
        context.host_function(crate::host::shared_vm::operation("netServerReference"))?;
    let socket_reference =
        context.host_function(crate::host::shared_vm::operation("netSocketReference"))?;
    let unref_timeout =
        context.host_function(crate::host::shared_vm::operation("netSocketUnrefTimeout"))?;
    let socket_options =
        context.host_function(crate::host::shared_vm::operation("netSocketOptions"))?;
    let server_options =
        context.host_function(crate::host::shared_vm::operation("netServerOptions"))?;
    let fin = context.host_function_with_data(
        crate::host::shared_vm::operation("netSocketWriteAfterFin"),
        duplex,
    )?;
    let facade = context.evaluate_script_rooted(quench_js_check::checked_js!(
        "(operation)=>function writeAfterFIN(chunk,encoding,cb){'use strict';return operation(this,chunk,encoding,cb);}"
    ), "node:net/write-after-fin.js")?;
    let undefined = context.undefined();
    let fin = context.call_rooted(facade, undefined, &[fin])?;
    let readable_end = context.host_function_with_data(
        crate::host::shared_vm::operation("netSocketReadableEnd"),
        fin,
    )?;
    let facade = context.evaluate_script_rooted(
        quench_js_check::checked_js!(
            "(operation)=>function onReadableStreamEnd(){'use strict';return operation(this);}"
        ),
        "node:net/readable-end.js",
    )?;
    let readable_end = context.call_rooted(facade, undefined, &[readable_end])?;
    let init_end = context.host_function(crate::host::shared_vm::operation("netSocketInitEnd"))?;
    let emitter_api = context
        .host_mut()
        .shared_state()
        .borrow()
        .event_targets_api
        .ok_or_else(|| RootedError::host("EventEmitter configuration missing"))?;
    let key = context.string_rooted("emitterCtor");
    let emitter = context.get_property_rooted(emitter_api, key)?;
    let init_server_emitter = context.host_function_with_data(
        crate::host::shared_vm::operation("netServerInitEmitter"),
        emitter,
    )?;
    let internal_net = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "net.js",
        "internal/net.js",
    )?;
    let [init_socket_options, set_no_delay, set_keep_alive, init_server_options] =
        super::net_socket_options::install(context, internal_net)?;
    let connections =
        context.host_function(crate::host::shared_vm::operation("netServerGetConnections"))?;
    let connections_factory = context.evaluate_script_rooted(
        quench_js_check::checked_js!(
            "operation=>function(cb){'use strict';return operation(this,cb);}"
        ),
        "node:net/get-connections.js",
    )?;
    let get_connections = context.call_rooted(connections_factory, undefined, &[connections])?;
    let initialize_read =
        context.host_function(crate::host::shared_vm::operation("netSocketInitializeRead"))?;
    let read_operation =
        context.host_function(crate::host::shared_vm::operation("netSocketRead"))?;
    let read_factory = context.evaluate_script_rooted(
        quench_js_check::checked_js!(
            "operation=>function(n){'use strict';return operation(this,n);}"
        ),
        "node:net/read.js",
    )?;
    let read_socket = context.call_rooted(read_factory, undefined, &[read_operation])?;
    let end_key = context.string_rooted("end");
    let prototype_key = context.string_rooted("prototype");
    let prototype = context.get_property_rooted(duplex, prototype_key)?;
    let original_end = context.get_property_rooted(prototype, end_key)?;
    let end_data = context.array_rooted(&[original_end, byte_length])?;
    let end_stream = context.host_function_with_data(
        crate::host::shared_vm::operation("netSocketEndStream"),
        end_data,
    )?;
    let normalized_args_key = context.string_rooted("normalizedArgsSymbol");
    let normalized_args_symbol = context.get_property_rooted(internal_net, normalized_args_key)?;
    let async_hooks = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "async_hooks.js",
        "internal/async_hooks.js",
    )?;
    let symbols_key = context.string_rooted("symbols");
    let symbols = context.get_property_rooted(async_hooks, symbols_key)?;
    let owner_symbol_key = context.string_rooted("owner_symbol");
    let owner_symbol = context.get_property_rooted(symbols, owner_symbol_key)?;
    let async_id_symbol_key = context.string_rooted("async_id_symbol");
    let async_id_symbol = context.get_property_rooted(symbols, async_id_symbol_key)?;
    let tcp_binding = super::tcp_binding::binding(context)?;
    let pipe_binding = super::pipe_binding::binding(context)?;
    let stream_binding = super::stream_binding::binding(context)?;
    let factory = context.evaluate_script_rooted(NET_MODULE_FACTORY, "node:net/module.js")?;
    let undefined = context.undefined();
    let surface = context.call_rooted(
        factory,
        undefined,
        &[
            duplex,
            connect,
            write,
            end,
            destroy,
            encoding,
            listen,
            close,
            address,
            account,
            endpoint,
            reference,
            socket_reference,
            unref_timeout,
            socket_options,
            server_options,
            init_end,
            readable_end,
            init_server_emitter,
            init_socket_options,
            set_no_delay,
            set_keep_alive,
            init_server_options,
            get_connections,
            initialize_read,
            read_socket,
            end_stream,
            normalized_args_symbol,
            owner_symbol,
            async_id_symbol,
            pipe_binding,
            stream_binding,
        ],
    )?;
    let options_data = context
        .host_mut()
        .shared_state()
        .borrow()
        .net_socket_option_keys
        .ok_or_else(|| RootedError::host("socket option metadata missing"))?;
    let key = context.string_rooted("module");
    context.define_data_property_rooted(options_data, key, surface, false, false, false)?;
    let key = context.string_rooted("Server");
    let server = context.get_property_rooted(surface, key)?;
    let key = context.string_rooted("prototype");
    let server_prototype = context.get_property_rooted(server, key)?;
    let emitter_prototype = context.get_property_rooted(emitter, key)?;
    context.set_prototype_rooted(server_prototype, emitter_prototype)?;
    context.set_prototype_rooted(server, emitter)?;
    super::net_server_listen::install(context, server_prototype)?;
    for name in [
        "Socket",
        "Stream",
        "Server",
        "BlockList",
        "SocketAddress",
        "connect",
        "createConnection",
        "createServer",
        "_normalizeArgs",
    ] {
        let key = context.string_rooted(name);
        let value = context.get_property_rooted(surface, key)?;
        if !context.set_property_rooted(module, key, value, module)? {
            return Err(RootedError::host(format!("cannot install net.{name}")));
        }
    }
    let uv_einval = context.number(-22.0);
    let create_server_handle_factory = context.evaluate_script_rooted(
        quench_js_check::checked_js!(
            r#"(() => {
  return function createServerHandleFactory({ TCP, constants }, UV_EINVAL) {
  function bind(address, port, addressType, flags) {
    const handle = new TCP(constants.SERVER);
    const status = addressType === 6 ?
      handle.bind6(address, port, flags) : handle.bind(address, port, flags);
    if (status !== 0) {
      handle.close();
      return status;
    }
    return handle;
  }
  return function createServerHandle(address, port, addressType, fd, flags) {
    if (typeof fd === 'number' && fd >= 0) return UV_EINVAL;
    if (!address) {
      const ipv6 = bind('::', port, 6, flags);
      if (typeof ipv6 !== 'number') return ipv6;
      return bind('0.0.0.0', port, 4, flags);
    }
    return bind(address, port, addressType, flags);
  };
  };
})()"#
        ),
        "node:net/create-server-handle.js",
    )?;
    let create_server_handle = context.call_rooted(
        create_server_handle_factory,
        undefined,
        &[tcp_binding, uv_einval],
    )?;
    let key = context.string_rooted("_createServerHandle");
    if !context.set_property_rooted(module, key, create_server_handle, module)? {
        return Err(RootedError::host("cannot install net._createServerHandle"));
    }
    let socket_key = context.string_rooted("Socket");
    let socket_constructor = context.get_property_rooted(surface, socket_key)?;
    super::net_socket_address::install(context, socket_constructor)?;
    let retained = context.retain(socket_constructor)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .net_socket_constructor = Some(retained);
    Ok(module)
}

pub(crate) fn is_ip(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let ipv4 = is_ipv4(context, receiver, args)?;
    if context.truthy_rooted(ipv4)? {
        return Ok(context.number(f64::from(super::net_config::IPV4_FAMILY)));
    }
    let ipv6 = is_ipv6(context, receiver, args)?;
    let family = if context.truthy_rooted(ipv6)? {
        super::net_config::IPV6_FAMILY
    } else {
        super::net_config::ANY_FAMILY
    };
    Ok(context.number(f64::from(family)))
}

pub(crate) fn is_ipv4(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = args.first().copied().unwrap_or_else(|| context.undefined());
    let input = context.to_string(input)?;
    Ok(context.boolean(input.parse::<Ipv4Addr>().is_ok()))
}

pub(crate) fn is_ipv6(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = args.first().copied().unwrap_or_else(|| context.undefined());
    let input = context.to_string(input)?;
    let ipv6 = parse_ip(&input).is_some_and(|ip| ip.is_ipv6());
    Ok(context.boolean(ipv6))
}

pub(super) fn parse_ip(input: &str) -> Option<IpAddr> {
    if let Ok(address) = input.parse::<IpAddr>() {
        return Some(address);
    }
    let (address, zone) = input.rsplit_once('%')?;
    if zone.is_empty()
        || !zone
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'-'))
    {
        return None;
    }
    address.parse::<Ipv6Addr>().ok().map(IpAddr::V6)
}

pub(crate) fn connect_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("net.connect requires a socket"))?;
    let message = context.object_rooted()?;
    let socket_key = context.string_rooted("socket");
    context.define_data_property_rooted(message, socket_key, socket, true, true, true)?;
    crate::modules::diagnostics_channel_shared_vm::publish_named(
        context,
        "net.client.socket",
        message,
    )?;
    #[cfg(unix)]
    if let Some(path) = args
        .get(1)
        .copied()
        .map(|root| context.string_text(root))
        .transpose()?
        .flatten()
    {
        let shared = context.host_mut().shared_state();
        let bind_path = resolve_local_path(&shared.borrow().cwd, &path);
        let id = connect_local(&mut shared.borrow_mut().tcp, bind_path, path)
            .map_err(RootedError::host)?;
        return register_socket(context, socket, id);
    }
    let port = args
        .get(1)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_number)
        .filter(|port| port.fract() == 0.0 && (0.0..=65535.0).contains(port))
        .ok_or_else(|| RootedError::host("invalid TCP port"))? as u16;
    let host = match args.get(2).copied() {
        Some(host) => context
            .string_text(host)?
            .unwrap_or_else(|| "localhost".to_owned()),
        None => "localhost".to_owned(),
    };
    let options = args.get(3).copied().unwrap_or_else(|| context.undefined());
    if parse_ip(&host).is_none() {
        let host_root = context.string_rooted(&host);
        let port_root = context.number(f64::from(port));
        return super::net_lookup_shared_vm::start(context, socket, host_root, port_root, options);
    }
    let shared = context.host_mut().shared_state();
    let id = reserve_lookup(&mut shared.borrow_mut().tcp).map_err(RootedError::host)?;
    register_socket(context, socket, id)?;
    super::net_options_shared_vm::connect_family_options(context, options)?;
    let (local_address, local_port) =
        super::net_options_shared_vm::connect_source(context, options)?;
    connect_resolved(
        &mut shared.borrow_mut().tcp,
        id,
        vec![host],
        port,
        local_address,
        local_port,
    )
    .map_err(RootedError::host)?;
    Ok(socket)
}

/// Count bytes at stream admission, once. The stream's existing private
/// accounted handoff prevents pending chunks from being counted again.
pub(crate) fn account_write_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    if args.get(3).copied().is_some_and(|value| {
        context
            .rooted_value(value)
            .is_some_and(|value| value.as_bool() == Some(true))
    }) {
        return Ok(undefined);
    }
    let byte_length = context.host_function_data()?;
    account_write(
        context,
        byte_length,
        args[0],
        args.get(1).copied().unwrap_or(undefined),
        args.get(2).copied().unwrap_or(undefined),
    )
}

fn account_write(
    context: &mut NativeContext<'_, NodeHost>,
    byte_length: RootId,
    socket: RootId,
    chunk: RootId,
    encoding: RootId,
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();

    let count = if context.string_text(chunk)?.is_some() {
        let length = context.call_rooted(byte_length, undefined, &[chunk, encoding])?;
        context
            .rooted_value(length)
            .and_then(|value| value.as_number())
            .unwrap_or_default()
    } else if context
        .rooted_value(chunk)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        0.0
    } else {
        let key = context.string_rooted("length");
        let length = context.get_property_rooted(chunk, key)?;
        context
            .rooted_value(length)
            .and_then(|value| value.as_number())
            .unwrap_or_default()
    };
    let key = context.string_rooted("bytesWritten");
    let old = context.get_property_rooted(socket, key)?;
    let old = context
        .rooted_value(old)
        .and_then(|value| value.as_number())
        .unwrap_or_default();
    let value = context.number(old + count);
    context.set_property_rooted(socket, key, value, socket)?;
    Ok(undefined)
}

pub(crate) fn end_stream(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args[0];
    let data = context.host_function_data()?;
    let key = context.string_rooted("0");
    let original = context.get_property_rooted(data, key)?;
    let key = context.string_rooted("_writableState");
    let state = context.get_property_rooted(socket, key)?;
    let key = context.string_rooted("ending");
    let ending = context.get_property_rooted(state, key)?;
    let key = context.string_rooted("destroyed");
    let destroyed = context.get_property_rooted(state, key)?;
    let accepted = !context.truthy_rooted(ending)? && !context.truthy_rooted(destroyed)?;
    let result = context.call_rooted(original, socket, &args[1..])?;
    if accepted {
        let key = context.string_rooted("errored");
        let error = context.get_property_rooted(state, key)?;
        if !context.truthy_rooted(error)? {
            let key = context.string_rooted("1");
            let byte_length = context.get_property_rooted(data, key)?;
            let missing = context.undefined();
            account_write(
                context,
                byte_length,
                socket,
                args.get(1).copied().unwrap_or(missing),
                args.get(2).copied().unwrap_or(missing),
            )?;
        }
    }
    Ok(result)
}

pub(crate) fn write_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("net.Socket.write requires a socket"))?;
    let id = socket_id(context, socket)?;
    let raw = is_raw_wrapped_socket(context, socket, id)?;
    let bytes = args
        .get(1)
        .copied()
        .map(|value| {
            context
                .view_bytes_rooted(value)
                .unwrap_or_else(|| context.to_string(value).unwrap_or_default().into_bytes())
        })
        .unwrap_or_default();
    let callback = args
        .get(2)
        .copied()
        .ok_or_else(|| RootedError::host("socket write callback is missing"))?;
    if !bytes.is_empty() {
        crate::modules::tls_shared_vm::socket::start_for_write(context, socket)?;
    }
    let state = context.host_mut().shared_state();
    if !state.borrow().net_sockets.contains_key(&id) {
        return Err(RootedError::host("socket is closed"));
    }
    let callback = context.retain(callback)?;
    state
        .borrow_mut()
        .net_sockets
        .get_mut(&id)
        .expect("checked live socket")
        .write_callbacks
        .push_back(callback);
    // The transport owns accepted writes from admission onwards. In particular
    // writeQueueSize excludes bytes synchronously accepted by the OS.
    let connected = is_connected(&state.borrow().tcp, id);
    if connected {
        write_with_completion(&mut state.borrow_mut().tcp, id, &bytes, raw)
            .map_err(RootedError::host)?;
    } else {
        // Connecting sockets keep their existing deferred submission path.
        context.host_mut().net_pending_writes.push((id, bytes, raw));
    }
    Ok(context.boolean(true))
}

pub(crate) fn end_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("net.Socket.end requires a socket"))?;
    // A fresh Socket has no transport to shut down; the real Writable final
    // callback still completes its writable side.
    let key = context.string_rooted("__quenchNetSocketId");
    let id = context.get_property_rooted(socket, key)?;
    if context.rooted_value(id).is_some_and(Value::is_undefined) {
        return Ok(socket);
    }
    let id = socket_id(context, socket)?;
    let raw = is_raw_wrapped_socket(context, socket, id)?;
    context.host_mut().net_pending_ends.push((id, raw));
    Ok(socket)
}

pub(crate) fn destroy_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("net.Socket.destroy requires a socket"))?;
    let error = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let had_error = context.truthy_rooted(error)?;
    let value = context.boolean(had_error);
    let key = context.host_function_data()?;
    context.define_data_property_rooted(socket, key, value, true, true, true)?;
    let callback = args
        .get(2)
        .copied()
        .ok_or_else(|| RootedError::host("socket destroy callback missing"))?;
    let id = match socket_id(context, socket) {
        Ok(id) => id,
        Err(_) => {
            let undefined = context.undefined();
            context.call_rooted(callback, undefined, &[error])?;
            let callback = context
                .host_mut()
                .shared_state()
                .borrow()
                .net_socket_close_completed
                .ok_or_else(|| RootedError::host("socket close callback missing"))?;
            let physical = context.boolean(false);
            super::shared_event_loop::queue_next_tick(
                context,
                callback,
                undefined,
                &[socket, physical],
            )?;
            super::net_server_admission::socket_destroyed(context, socket, None)?;
            return Ok(socket);
        }
    };
    context
        .host_mut()
        .net_pending_writes
        .retain(|(socket, _, _)| *socket != id);
    context
        .host_mut()
        .net_pending_ends
        .retain(|(socket, _)| *socket != id);
    let state = context.host_mut().shared_state();
    let was_connected = is_connected(&state.borrow().tcp, id);
    let reset_key = context.string_rooted("resetAndClosing");
    let reset = context.get_property_rooted(socket, reset_key)?;
    if context.truthy_rooted(reset)? {
        reset_socket(&state.borrow().tcp, id)
            .map_err(|error| RootedError::host(error.to_string()))?;
    }
    close_socket(&mut state.borrow_mut().tcp, id);
    context
        .host_mut()
        .net_pending_destroys
        .push((id, was_connected));
    let key = context.string_rooted("_handle");
    let null = context.null();
    context.set_property_rooted(socket, key, null, socket)?;
    let key = context.string_rooted("_sockname");
    context.set_property_rooted(socket, key, null, socket)?;
    let undefined = context.undefined();
    context.call_rooted(callback, undefined, &[error])?;
    super::net_server_admission::socket_destroyed(context, socket, Some(id))?;
    Ok(socket)
}

pub(crate) fn set_encoding_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("net.Socket.setEncoding requires a socket"))?;
    // `net.Socket({ fd })` can adopt a native Pipe handle that is not one of
    // the TCP transports tracked by this facade. Readable.setEncoding still
    // performs its own decoding; only the native TCP read queue needs this
    // encoding hint for its byte accounting.
    let Ok(id) = socket_id(context, socket) else {
        return Ok(context.undefined());
    };
    let encoding = args
        .get(1)
        .copied()
        .map(|value| context.to_string(value))
        .transpose()?;
    if let Some(socket) = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .net_sockets
        .get_mut(&id)
    {
        socket.encoding = encoding;
    }
    Ok(context.undefined())
}

#[cfg(unix)]
pub(super) fn listen_local_server(
    context: &mut NativeContext<'_, NodeHost>,
    server: RootId,
    path: &str,
) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    let cwd = shared.borrow().cwd.clone();
    let bind_path = resolve_local_path(&cwd, path);
    let cleanup_path = cleanup_local_path(&cwd, path);
    let result = bind_path.and_then(|bind_path| {
        cleanup_path.and_then(|cleanup_path| {
            listen_local(
                &mut shared.borrow_mut().tcp,
                bind_path,
                path.to_owned(),
                cleanup_path,
                DEFAULT_TCP_LISTEN_BACKLOG,
            )
        })
    });
    match result {
        Ok(id) => register_server(context, server, id),
        Err(error) => queue_listen_error(context, server, error, path),
    }
}

/// Bind a Unix-domain listener for a consumer that owns its own public server
/// bookkeeping (for example HTTP). Unlike `listen_local_server`, this leaves
/// registration of the accepted listener to the caller.
#[cfg(unix)]
pub(crate) fn listen_local_endpoint(
    context: &mut NativeContext<'_, NodeHost>,
    path: &str,
    backlog: i32,
) -> Result<u64, String> {
    let shared = context.host_mut().shared_state();
    let cwd = shared.borrow().cwd.clone();
    let bind_path = resolve_local_path(&cwd, path).map_err(|error| error.to_string())?;
    let cleanup_path = cleanup_local_path(&cwd, path).map_err(|error| error.to_string())?;
    let result = {
        let mut state = shared.borrow_mut();
        listen_local(
            &mut state.tcp,
            bind_path,
            path.to_owned(),
            cleanup_path,
            backlog,
        )
    };
    result.map_err(|error| error.to_string())
}

pub(crate) fn server_address_operation(
    context: &mut NativeContext<'_, NodeHost>,
    server: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(id) = optional_server_id(context, server)? else {
        return Ok(context.null());
    };
    let shared = context.host_mut().shared_state();
    let local = listener_path(&shared.borrow().tcp, id).map(str::to_owned);
    if let Some(path) = local {
        return Ok(context.string_rooted(&path));
    }
    let address = address(&shared.borrow().tcp, id);
    let Some(address) = address else {
        return Ok(context.null());
    };
    let object = context.object_rooted()?;
    let address_value = context.string_rooted(&address.ip().to_string());
    let key = context.string_rooted("address");
    context.set_property_rooted(object, key, address_value, object)?;
    let family = context.string_rooted(if address.ip().is_ipv4() {
        "IPv4"
    } else {
        "IPv6"
    });
    let key = context.string_rooted("family");
    context.set_property_rooted(object, key, family, object)?;
    let port = context.number(address.port() as f64);
    let key = context.string_rooted("port");
    context.set_property_rooted(object, key, port, object)?;
    Ok(object)
}

pub(super) fn optional_server_id(
    context: &mut NativeContext<'_, NodeHost>,
    server: RootId,
) -> Result<Option<u64>, RootedError> {
    let key = context.string_rooted("__quenchNetServerId");
    let value = context.get_property_rooted(server, key)?;
    Ok(context
        .rooted_value(value)
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && id.fract() == 0.0 && *id >= 0.0)
        .map(|id| id as u64))
}

pub(super) fn socket_id(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
) -> Result<u64, RootedError> {
    let key = context.string_rooted("__quenchNetSocketId");
    let value = context.get_property_rooted(socket, key)?;
    context
        .rooted_value(value)
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && id.fract() == 0.0 && *id >= 0.0)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("net socket identifier is invalid"))
}

/// Return the live descriptor for a TCP socket supplied as child stdio.
/// `child_process` duplicates this descriptor before handing it to the child,
/// so the parent socket remains usable while the child writes to its endpoint.
#[cfg(unix)]
pub(crate) fn child_stdio_socket_descriptor(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
) -> Result<Option<i32>, RootedError> {
    let Ok(id) = socket_id(context, socket) else {
        return Ok(None);
    };
    let shared = context.host_mut().shared_state();
    let state = shared.borrow();
    let fd = native_tcp_fd(&state.tcp, id);
    Ok((fd >= 0).then_some(fd))
}

#[cfg(not(unix))]
pub(crate) fn child_stdio_socket_descriptor(
    _: &mut NativeContext<'_, NodeHost>,
    _: RootId,
) -> Result<Option<i32>, RootedError> {
    Ok(None)
}

pub(crate) fn get_timeout(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let timeout = context
        .host_mut()
        .shared_state()
        .borrow()
        .net_auto_select_family_attempt_timeout;
    Ok(context.number(timeout as f64))
}

pub(crate) fn set_timeout(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let timeout = super::net_options_shared_vm::attempt_timeout(context, value, "value")?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .net_auto_select_family_attempt_timeout = timeout;
    Ok(context.undefined())
}

/// Adapt the generic stream completion to Socket's latched destruction outcome.

#[cfg(unix)]
fn listen_local(
    transport: &mut Transport,
    path: std::path::PathBuf,
    display: String,
    cleanup_path: Option<std::path::PathBuf>,
    backlog: i32,
) -> std::io::Result<u64> {
    let (socket, unlink) = bind_local_address(&path, &display, cleanup_path)?;
    use std::os::fd::AsRawFd;
    if unsafe { libc::listen(socket.as_raw_fd(), backlog) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let socket = ListenerSocket::Local {
        listener: socket,
        path: unlink,
        display,
    };
    if let ListenerSocket::Local { listener, .. } = &socket {
        listener.set_nonblocking(true)?;
    }
    let id = transport.id().map_err(std::io::Error::other)?;
    transport.listeners.insert(id, Listener::new(socket));
    Ok(id)
}
#[cfg(unix)]
fn connect_local(
    transport: &mut Transport,
    path: std::io::Result<std::path::PathBuf>,
    display: String,
) -> Result<u64, String> {
    let id = transport.id()?;
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name(format!("quench-node-local-{id}"))
        .spawn(move || {
            let result = path
                .and_then(|path| connect_local_address(&path, &display))
                .and_then(|stream| {
                    stream.set_nonblocking(true)?;
                    Ok(SocketStream::Plain(RawStream::Local(stream)))
                });
            let _ = sender.send(result.map_err(|error| ConnectFailure::Local {
                error,
                path: display,
            }));
        })
        .map_err(|error| error.to_string())?;
    transport.connecting.insert(
        id,
        ConnectingSocket {
            completion: ConnectCompletion::NativeRequest(receiver),
            referenced: true,
            protocol: SocketProtocol::Local,
            options: TcpOptions::default(),
            read_demand: ReadDemand::Active,
            pending_write: Vec::new(),
        },
    );
    Ok(id)
}
pub(crate) fn listener_path(transport: &Transport, id: u64) -> Option<&str> {
    match transport.listeners.get(&id)?.socket()? {
        #[cfg(unix)]
        ListenerSocket::Local { display, .. } => Some(display),
        _ => None,
    }
}

pub(super) fn register_socket(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
    id: u64,
) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    let key = context.string_rooted("__quenchNetSocketId");
    let id_value = context.number(id as f64);
    if !context.set_property_rooted(socket, key, id_value, socket)? {
        return Err(RootedError::host("cannot tag net socket"));
    }
    let retained = context.retain(socket)?;
    shared.borrow_mut().net_sockets.insert(
        id,
        crate::host::node_host::NetSocket {
            root: retained,
            encoding: None,
            parent_server: None,
            write_callbacks: VecDeque::new(),
        },
    );
    // A public Socket now owns delivery even when it adopts an active raw handle.
    super::stream_binding::release_reader(context, id);
    super::net_reference_shared_vm::install_socket_handle(context, id, socket)?;
    Ok(socket)
}

/// Duplicate a live plain TCP socket for Node's `child.send(message, socket,
/// { keepOpen: true })` IPC handle transfer. Passing the duplicate with
/// SCM_RIGHTS leaves the sender's socket usable, as Node's keep-open mode does.
#[cfg(unix)]
pub(crate) fn ipc_socket_handle(
    context: &mut NativeContext<'_, NodeHost>,
    handle: RootId,
) -> Result<Option<std::os::fd::OwnedFd>, RootedError> {
    use std::os::fd::AsFd;

    let Ok(id) = socket_id(context, handle) else {
        return Ok(None);
    };
    let shared = context.host_mut().shared_state();
    let state = shared.borrow();
    let Some(socket) = state.tcp.sockets.get(&id) else {
        return Ok(None);
    };
    let SocketStream::Plain(RawStream::Tcp(stream)) = &socket.stream else {
        return Ok(None);
    };
    stream
        .as_fd()
        .try_clone_to_owned()
        .map(Some)
        .map_err(|error| RootedError::host(format!("cannot duplicate TCP socket: {error}")))
}

/// Close the sender's Socket after a successful default (non-keep-open) IPC
/// transfer. This is called only after the descriptor is on the IPC channel.
#[cfg(unix)]
pub(crate) fn close_ipc_socket(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
) -> Result<(), RootedError> {
    if socket_id(context, socket).is_err() {
        return Ok(());
    }
    let key = context.string_rooted("destroy");
    let destroy = context.get_property_rooted(socket, key)?;
    context.call_rooted(destroy, socket, &[])?;
    Ok(())
}

pub(super) fn register_server(
    context: &mut NativeContext<'_, NodeHost>,
    server: RootId,
    listener: u64,
) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    let key = context.string_rooted("__quenchNetServerId");
    let value = context.number(listener as f64);
    if !context.set_property_rooted(server, key, value, server)? {
        return Err(RootedError::host("cannot tag net server"));
    }
    let retained = context.retain(server)?;
    shared.borrow_mut().net_servers.insert(
        listener,
        crate::host::node_host::NetServer {
            root: retained,
            listener,
            connections: Default::default(),
            closing: false,
            listening_pending: true,
        },
    );
    super::net_reference_shared_vm::install(context, listener, server)?;
    Ok(server)
}

#[cfg(unix)]
fn queue_listen_error(
    context: &mut NativeContext<'_, NodeHost>,
    server: RootId,
    os_error: std::io::Error,
    path: &str,
) -> Result<RootId, RootedError> {
    let errno = os_error.raw_os_error().map(|value| -f64::from(value));
    let code = errno
        .map(crate::modules::util_system_error_shared_vm::system_error_name)
        .unwrap_or_else(|| "UNKNOWN".to_owned());
    let reason = os_error.to_string();
    let reason = reason
        .split(" (os error")
        .next()
        .unwrap_or(&reason)
        .to_lowercase();
    let error = context.error_rooted(&format!("listen {code}: {reason} {path}"))?;
    for (name, text) in [
        ("code", code.as_str()),
        ("syscall", "listen"),
        ("address", path),
    ] {
        let key = context.string_rooted(name);
        let value = context.string_rooted(text);
        context.define_data_property_rooted(error, key, value, true, true, true)?;
    }
    if let Some(errno) = errno {
        let key = context.string_rooted("errno");
        let value = context.number(errno);
        context.define_data_property_rooted(error, key, value, true, true, true)?;
    }
    let key = context.string_rooted("port");
    let pipe_port = context.number(-1.0);
    context.define_data_property_rooted(error, key, pipe_port, true, true, true)?;
    let callback = context.host_function(crate::host::shared_vm::operation("netListenError"))?;
    let receiver = context.undefined();
    super::shared_event_loop::queue_next_tick(context, callback, receiver, &[server, error])?;
    Ok(server)
}

pub(crate) fn listen_error(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let server = args[0];
    let error = args[1];
    let key = context.string_rooted("listening");
    let no = context.boolean(false);
    context.set_property_rooted(server, key, no, server)?;
    let key = context.string_rooted("emit");
    let emit = context.get_property_rooted(server, key)?;
    let event = context.string_rooted("error");
    context.call_rooted(emit, server, &[event, error])?;
    Ok(context.undefined())
}

#[cfg(unix)]
fn validate_local_path(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    // libuv UV_PIPE_NO_TRUNCATE rejects paths which do not fit sockaddr_un.
    let address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    let bytes = path.as_os_str().as_bytes();
    if bytes.len() >= address.sun_path.len() || bytes.contains(&0) {
        return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
    }
    Ok(())
}

#[cfg(unix)]
fn bind_local_address(
    path: &std::path::Path,
    _display: &str,
    cleanup_path: Option<std::path::PathBuf>,
) -> std::io::Result<(UnixListener, Option<std::path::PathBuf>)> {
    #[cfg(target_os = "linux")]
    if let Some(name) = _display.strip_prefix('\0') {
        use std::os::linux::net::SocketAddrExt;
        let address = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes())
            .map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
        return UnixListener::bind_addr(&address).map(|listener| (listener, None));
    }
    validate_local_path(path)?;
    UnixListener::bind(path).map(|listener| (listener, cleanup_path))
}
#[cfg(unix)]
fn connect_local_address(path: &std::path::Path, _display: &str) -> std::io::Result<UnixStream> {
    #[cfg(target_os = "linux")]
    if let Some(name) = _display.strip_prefix('\0') {
        use std::os::linux::net::SocketAddrExt;
        let address = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes())
            .map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
        return UnixStream::connect_addr(&address);
    }
    validate_local_path(path)?;
    UnixStream::connect(path)
}

#[cfg(unix)]
fn resolve_local_path(
    cwd: &super::process_state::ProcessCwd,
    path: &str,
) -> std::io::Result<std::path::PathBuf> {
    let path = std::path::Path::new(path);
    if path.is_absolute()
        || path.as_os_str().is_empty()
        || path.as_os_str().as_encoded_bytes().first() == Some(&0)
    {
        return Ok(path.to_owned());
    }
    // ProcessCwd is virtual, so the host process cwd may differ from the cwd
    // observed by Node. Resolve through its retained directory descriptor; on
    // Linux this produces a short /proc/self/fd path and preserves directory
    // identity if the directory is renamed. Keep the original relative path
    // when the resolved host path cannot fit sockaddr_un, since some valid
    // relative socket names are shorter than their absolute form.
    let resolved = cwd.resolve_io_path(path);
    let address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if resolved.as_os_str().as_encoded_bytes().len() < address.sun_path.len() {
        return Ok(resolved);
    }
    // The relative name itself may still fit sockaddr_un.
    Ok(path.to_owned())
}

#[cfg(unix)]
fn cleanup_local_path(
    cwd: &super::process_state::ProcessCwd,
    path: &str,
) -> std::io::Result<Option<std::path::PathBuf>> {
    let path = std::path::Path::new(path);
    if path.as_os_str().as_encoded_bytes().first() == Some(&0) {
        return Ok(None);
    }
    if path.is_absolute() {
        return Ok(Some(path.to_owned()));
    }
    cwd.current_path()
        .map(|directory| Some(directory.join(path)))
}

pub(crate) fn listener_reference(transport: &mut Transport, id: u64, referenced: bool) {
    if let Some(listener) = transport.listeners.get_mut(&id) {
        listener.referenced = referenced;
    }
}
pub(crate) fn listener_has_ref(transport: &Transport, id: u64) -> Option<bool> {
    transport
        .listeners
        .get(&id)
        .map(|listener| listener.referenced)
}

pub(crate) fn socket_reference(transport: &mut Transport, id: u64, referenced: bool) {
    if let Some(socket) = transport.connecting.get_mut(&id) {
        socket.referenced = referenced;
    } else if let Some(socket) = transport.sockets.get_mut(&id) {
        socket.referenced = referenced;
    } else if let Some(socket) = transport
        .closing_sockets
        .iter_mut()
        .find_map(|(existing, socket)| (*existing == CloseTarget::Stream(id)).then_some(socket))
    {
        socket.referenced = referenced;
    }
}
pub(crate) fn socket_has_ref(transport: &Transport, id: u64) -> Option<bool> {
    transport
        .connecting
        .get(&id)
        .map(|socket| socket.referenced)
        .or_else(|| transport.sockets.get(&id).map(|socket| socket.referenced))
        .or_else(|| {
            transport
                .closing_sockets
                .iter()
                .find_map(|(existing, socket)| {
                    (*existing == CloseTarget::Stream(id)).then_some(socket.referenced)
                })
        })
}

pub(crate) fn close_completed(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args[0];
    let key = context.host_function_data()?;
    let outcome = context.get_property_rooted(socket, key)?;
    let emit = super::event_constructor_shared_vm::get(context, socket, "emit")?;
    let event = context.string_rooted("close");
    let physical = args.get(1).copied().unwrap_or_else(|| context.undefined());
    if context.truthy_rooted(physical)? {
        context.call_rooted(emit, socket, &[event, outcome])?;
    } else {
        context.call_rooted(emit, socket, &[event])?;
    }
    Ok(context.undefined())
}

pub(crate) enum EncryptError {
    OpenSsl(openssl::error::ErrorStack),
    NotConnected,
    AlreadyEncrypted,
}
pub(crate) fn encrypt(
    transport: &mut Transport,
    id: u64,
    ssl: openssl::ssl::Ssl,
    role: encrypted::Role,
    started: bool,
) -> Result<(), EncryptError> {
    let Some(mut socket) = transport.sockets.remove(&id) else {
        return Err(EncryptError::NotConnected);
    };
    let raw = match socket.stream {
        SocketStream::Plain(raw) => raw,
        encrypted @ SocketStream::Encrypted(_) => {
            socket.stream = encrypted;
            transport.sockets.insert(id, socket);
            return Err(EncryptError::AlreadyEncrypted);
        }
    };
    let referenced = socket.referenced;
    match encrypted::EncryptedStream::new(ssl, raw, role, started) {
        Ok(tls) => {
            socket.stream = SocketStream::Encrypted(tls);
            transport.sockets.insert(id, socket);
            Ok(())
        }
        Err(error) => {
            request_socket_close(transport, id, referenced);
            Err(EncryptError::OpenSsl(error))
        }
    }
}
pub(crate) fn authorize(transport: &mut Transport, id: u64) {
    if let Some(SharedSocket {
        stream: SocketStream::Encrypted(tls),
        ..
    }) = transport.sockets.get_mut(&id)
    {
        tls.authorize();
    }
}
pub(crate) fn start_tls(transport: &mut Transport, id: u64) {
    if let Some(SharedSocket {
        stream: SocketStream::Encrypted(tls),
        ..
    }) = transport.sockets.get_mut(&id)
    {
        tls.start();
    }
}
pub(crate) fn tls_is_dormant(transport: &Transport, id: u64) -> bool {
    transport.sockets.get(&id).is_some_and(
        |socket| matches!(&socket.stream, SocketStream::Encrypted(tls) if tls.dormant()),
    )
}
pub(crate) fn receive_tls(transport: &mut Transport, id: u64, bytes: &[u8]) {
    if let Some(SharedSocket {
        stream: SocketStream::Encrypted(tls),
        ..
    }) = transport.sockets.get_mut(&id)
    {
        tls.receive(bytes);
    }
}
pub(crate) fn tls_info(transport: &Transport, id: u64) -> Option<&openssl::ssl::SslRef> {
    match &transport.sockets.get(&id)?.stream {
        SocketStream::Encrypted(tls) => Some(tls.ssl()),
        _ => None,
    }
}

pub(crate) fn release_delivery(transport: &mut Transport, id: u64) {
    if let Some(socket) = transport.sockets.get_mut(&id) {
        socket.delivery = DeliveryPhase::Delivered;
    }
}
pub(crate) fn deliver_eof(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    context.call_rooted(args[0], receiver, &args[1..])?;
    let key = context.string_rooted("read");
    let read = context.get_property_rooted(receiver, key)?;
    let zero = context.number(0.0);
    context.call_rooted(read, receiver, &[zero])
}
pub(crate) fn deliver_data(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = context
        .rooted_value(args[1])
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("queued socket input owner is unavailable"))?
        as u64;
    let result = (|| {
        let guard_key = context.string_rooted("__quenchHttpFreeSocketGuard");
        let guard = context.get_property_rooted(receiver, guard_key)?;
        if context.is_callable_rooted(guard)? {
            context.call_rooted(guard, receiver, &[])?;
            return Ok(context.undefined());
        }
        let key = context.string_rooted("bytesRead");
        let previous = context.get_property_rooted(receiver, key)?;
        let count =
            context.arithmetic_rooted(quench_runtime::NativeArithmetic::Add, previous, args[2])?;
        context.set_property_rooted(receiver, key, count, receiver)?;
        let key = context.string_rooted("_quenchResetTimeout");
        let reset = context.get_property_rooted(receiver, key)?;
        context.call_rooted(reset, receiver, &[])?;
        let result = context.call_rooted(args[0], receiver, &args[3..])?;
        super::net_socket_read::delivered(context, receiver, result)?;
        Ok(result)
    })();
    release_delivery(
        &mut context.host_mut().shared_state().borrow_mut().tcp,
        socket,
    );
    result
}
enum ConnectionDelivery {
    Accepted,
    Connected,
}
pub(crate) fn deliver_connect(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    complete_connection_delivery(context, receiver, args, ConnectionDelivery::Connected)
}

pub(crate) fn deliver_connection(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    complete_connection_delivery(context, receiver, args, ConnectionDelivery::Accepted)
}

fn complete_connection_delivery(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
    delivery: ConnectionDelivery,
) -> Result<RootId, RootedError> {
    let callback = args[0];
    let socket = context
        .rooted_value(args[1])
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("queued socket owner is unavailable"))?
        as u64;
    {
        let shared = context.host_mut().shared_state();
        if let Some(socket) = shared.borrow_mut().tcp.sockets.get_mut(&socket) {
            socket.delivery = DeliveryPhase::DeliveringConnection;
        };
    }
    let result = (|| {
        if matches!(delivery, ConnectionDelivery::Connected) {
            super::net_socket_options::apply_connected(context, receiver)?;
        }
        let result = context.call_rooted(callback, receiver, &args[2..])?;
        if matches!(delivery, ConnectionDelivery::Connected) {
            super::net_socket_read::connected(context, receiver)?;
        }
        Ok(result)
    })();
    let shared = context.host_mut().shared_state();
    {
        let mut state = shared.borrow_mut();
        release_delivery(&mut state.tcp, socket);
    }
    result
}

pub(crate) fn readable_end(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args[0];
    let key = context.string_rooted("allowHalfOpen");
    let allow = context.get_property_rooted(socket, key)?;
    if !context.truthy_rooted(allow)? {
        let replacement = context.host_function_data()?;
        let key = context.string_rooted("write");
        context.set_property_rooted(socket, key, replacement, socket)?;
    }
    Ok(context.undefined())
}

pub(crate) fn write_after_fin(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args[0];
    let key = context.string_rooted("writableEnded");
    let ended = context.get_property_rooted(socket, key)?;
    if !context.truthy_rooted(ended)? {
        let duplex = context.host_function_data()?;
        let key = context.string_rooted("prototype");
        let prototype = context.get_property_rooted(duplex, key)?;
        let key = context.string_rooted("write");
        let original = context.get_property_rooted(prototype, key)?;
        return context.call_rooted(original, socket, &args[1..]);
    }
    let callback = if context.is_callable_rooted(args[2])? {
        args[2]
    } else {
        args[3]
    };
    let error = context.error_rooted("This socket has been ended by the other party")?;
    let key = context.string_rooted("code");
    let code = context.string_rooted("EPIPE");
    context.define_data_property_rooted(error, key, code, true, true, true)?;
    if context.is_callable_rooted(callback)? {
        let receiver = context.undefined();
        super::shared_event_loop::queue_next_tick(context, callback, receiver, &[error])?;
    }
    let key = context.string_rooted("destroy");
    let destroy = context.get_property_rooted(socket, key)?;
    context.call_rooted(destroy, socket, &[error])?;
    Ok(context.boolean(false))
}

pub(crate) fn initialize_end(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args[0];
    let key = context.string_rooted("allowHalfOpen");
    let allow = context.get_property_rooted(args[1], key)?;
    let allow = context.truthy_rooted(allow)?;
    let allow = context.boolean(allow);
    context.set_property_rooted(socket, key, allow, socket)?;
    let key = context.string_rooted("on");
    let on = context.get_property_rooted(socket, key)?;
    let event = context.string_rooted("end");
    context.call_rooted(on, socket, &[event, args[2]])?;
    Ok(context.undefined())
}

pub(crate) fn initialize_server_emitter(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let emitter = context.host_function_data()?;
    context.call_rooted(emitter, args[0], &[])
}

/// The accepted wrapper becomes observable when its original connection
/// callback begins. Read delivery stays blocked until that callback completes.
pub(crate) fn connection_delivery_started(transport: &Transport, id: u64) -> bool {
    transport
        .sockets
        .get(&id)
        .is_none_or(|socket| !matches!(socket.delivery, DeliveryPhase::Pending))
}

pub(crate) fn socket_is_tcp(transport: &Transport, id: u64) -> bool {
    transport
        .sockets
        .get(&id)
        .map(|socket| matches!(socket.stream.raw(), RawStream::Tcp(_)))
        .or_else(|| {
            transport
                .connecting
                .get(&id)
                .map(|socket| matches!(socket.protocol, SocketProtocol::Tcp))
        })
        .unwrap_or(false)
}
fn apply_tcp_keep_alive(stream: &TcpStream, options: TcpKeepAlive) -> std::io::Result<()> {
    let socket = socket2::SockRef::from(stream);
    socket.set_keepalive(options.enabled)?;
    if !options.enabled {
        return Ok(());
    }
    if options.delay == 0 || options.interval == 0 || options.count == 0 {
        return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
    }
    let keep_alive = socket2::TcpKeepalive::new()
        .with_time(std::time::Duration::from_secs(u64::from(options.delay)))
        .with_interval(std::time::Duration::from_secs(u64::from(options.interval)))
        .with_retries(options.count);
    socket.set_tcp_keepalive(&keep_alive)
}
fn apply_tcp_type_of_service(stream: &TcpStream, tos: u8) -> std::io::Result<()> {
    socket2::SockRef::from(stream).set_tos_v4(u32::from(tos))
}
pub(crate) fn tcp_no_delay(
    transport: &mut Transport,
    id: u64,
    enabled: bool,
) -> std::io::Result<()> {
    if let Some(socket) = transport.connecting.get_mut(&id) {
        socket.options.no_delay = Some(enabled);
        return Ok(());
    }
    match transport.sockets.get(&id).map(|socket| socket.stream.raw()) {
        Some(RawStream::Tcp(stream)) => stream.set_nodelay(enabled),
        _ => Err(std::io::Error::from_raw_os_error(libc::EBADF)),
    }
}
pub(crate) fn tcp_keep_alive(
    transport: &mut Transport,
    id: u64,
    options: TcpKeepAlive,
) -> std::io::Result<()> {
    if let Some(socket) = transport.connecting.get_mut(&id) {
        socket.options.keep_alive = Some(options);
        return Ok(());
    }
    match transport.sockets.get(&id).map(|socket| socket.stream.raw()) {
        Some(RawStream::Tcp(stream)) => apply_tcp_keep_alive(stream, options),
        _ => Err(std::io::Error::from_raw_os_error(libc::EBADF)),
    }
}

pub(crate) fn tcp_set_type_of_service(
    transport: &mut Transport,
    id: u64,
    tos: u8,
) -> std::io::Result<()> {
    if let Some(socket) = transport.connecting.get_mut(&id) {
        socket.options.type_of_service = Some(tos);
        return Ok(());
    }
    match transport.sockets.get(&id).map(|socket| socket.stream.raw()) {
        Some(RawStream::Tcp(stream)) => apply_tcp_type_of_service(stream, tos),
        _ => Err(std::io::Error::from_raw_os_error(libc::EBADF)),
    }
}

pub(crate) fn tcp_get_type_of_service(transport: &Transport, id: u64) -> std::io::Result<u32> {
    match transport.sockets.get(&id).map(|socket| socket.stream.raw()) {
        Some(RawStream::Tcp(stream)) => socket2::SockRef::from(stream).tos_v4(),
        _ => Err(std::io::Error::from_raw_os_error(libc::EBADF)),
    }
}

pub(crate) fn start_read(transport: &mut Transport, id: u64) -> std::io::Result<()> {
    let socket = transport
        .sockets
        .get_mut(&id)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EINVAL))?;
    if socket.read_ended {
        return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
    }
    if socket.read_demand == ReadDemand::Active {
        return Err(std::io::Error::from_raw_os_error(libc::EALREADY));
    }
    socket.read_demand = ReadDemand::Active;
    Ok(())
}
pub(crate) fn stop_read(transport: &mut Transport, id: u64) {
    if let Some(socket) = transport.sockets.get_mut(&id) {
        socket.read_demand = ReadDemand::Idle;
    }
    if let Some(socket) = transport.connecting.get_mut(&id) {
        socket.read_demand = ReadDemand::Idle;
    }
}

pub(crate) fn read_active(transport: &Transport, id: u64) -> bool {
    transport
        .sockets
        .get(&id)
        .map(|socket| socket.read_demand)
        .or_else(|| {
            transport
                .connecting
                .get(&id)
                .map(|socket| socket.read_demand)
        })
        == Some(ReadDemand::Active)
}

pub(crate) fn socket_has_pending_read(transport: &Transport, id: u64) -> bool {
    transport
        .sockets
        .get(&id)
        .is_some_and(|socket| socket.stream.has_pending_read())
}

fn poll_write_completions(socket: &mut SharedSocket, id: u64, events: &mut Vec<TransportEvent>) {
    while socket
        .completions
        .front()
        .is_some_and(|boundary| boundary.end <= socket.write_offset)
    {
        let boundary = socket
            .completions
            .pop_front()
            .expect("checked completion boundary");
        events.push(match boundary.owner {
            WriteOwner::Guest => TransportEvent::WriteComplete { socket: id },
            WriteOwner::Native(request) => TransportEvent::NativeWriteComplete { request },
        });
    }
}
pub(crate) fn native_tcp_create(transport: &mut Transport) -> std::io::Result<u64> {
    let socket = super::tcp_owned::TcpOwned::new()?;
    let id = transport.id().map_err(std::io::Error::other)?;
    transport.native_tcp_unbound.insert(id, socket);
    Ok(id)
}
pub(crate) fn native_tcp_bind(
    transport: &mut Transport,
    id: u64,
    address: SocketAddr,
    flags: u32,
) -> std::io::Result<()> {
    if transport.listeners.contains_key(&id) || transport.sockets.contains_key(&id) {
        return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
    }
    transport
        .native_tcp_unbound
        .get_mut(&id)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))?
        .bind(address, flags)
}
pub(crate) fn native_tcp_listen(
    transport: &mut Transport,
    id: u64,
    backlog: i32,
) -> std::io::Result<()> {
    if let Some(listener) = transport.listeners.get(&id).and_then(Listener::socket) {
        // Existing descriptor remains the only socket owner on repeat listen.
        return match listener {
            ListenerSocket::Tcp(listener) => socket2::SockRef::from(listener).listen(backlog),
            #[cfg(unix)]
            _ => Err(std::io::Error::from_raw_os_error(libc::EINVAL)),
        };
    }
    let socket = transport
        .native_tcp_unbound
        .get(&id)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))?;
    socket.start_listening(backlog)?;
    let socket = transport
        .native_tcp_unbound
        .remove(&id)
        .expect("validated live socket");
    let referenced = socket.referenced;
    let mut listener = Listener::new(ListenerSocket::Tcp(socket.into_listener()));
    listener.referenced = referenced;
    transport.listeners.insert(id, listener);
    Ok(())
}
/// Project the descriptor owned by the current native lifecycle state.
#[cfg(unix)]
pub(crate) fn native_tcp_fd(transport: &Transport, id: u64) -> i32 {
    use std::os::fd::AsRawFd;
    if let Some(socket) = transport.native_tcp_unbound.get(&id) {
        return socket.fd();
    }
    if let Some(socket) = transport.native_tcp_connecting.get(&id) {
        return socket.socket.fd();
    }
    if let Some(listener) = transport.listeners.get(&id).and_then(Listener::socket) {
        return match listener {
            ListenerSocket::Tcp(socket) => socket.as_raw_fd(),
            ListenerSocket::Local { listener, .. } => listener.as_raw_fd(),
        };
    }
    if let Some(socket) = transport.sockets.get(&id) {
        return match socket.stream.raw() {
            RawStream::Tcp(socket) => socket.as_raw_fd(),
            RawStream::Local(socket) => socket.as_raw_fd(),
        };
    }
    -libc::EBADF
}
#[cfg(not(unix))]
pub(crate) fn native_tcp_fd(_: &Transport, _: u64) -> i32 {
    -libc::EBADF
}
pub(crate) fn native_tcp_address(transport: &Transport, id: u64) -> std::io::Result<SocketAddr> {
    if let Some(socket) = transport.native_tcp_unbound.get(&id) {
        return socket.address();
    }
    if let Some(connection) = transport.native_tcp_connecting.get(&id) {
        return connection.socket.address();
    }
    if let Some(address) = address(transport, id) {
        return Ok(address);
    }
    socket_address(transport, id, false)
}
pub(crate) fn native_tcp_is_open(transport: &Transport, id: u64) -> bool {
    transport.native_tcp_connecting.contains_key(&id)
        || transport.native_tcp_unbound.contains_key(&id)
        || transport.listeners.contains_key(&id)
        || socket_is_open(transport, id)
}
pub(crate) fn native_tcp_close(transport: &mut Transport, id: u64) {
    // Removing the pre-listen/listener owner physically closes its descriptor.
    // Reuse the same close completion machine as connected endpoints.
    let referenced = transport
        .native_tcp_unbound
        .remove(&id)
        .map(|socket| socket.referenced)
        .or_else(|| {
            transport
                .native_tcp_connecting
                .remove(&id)
                .map(|connection| connection.socket.referenced)
        })
        .or_else(|| {
            transport
                .listeners
                .remove(&id)
                .map(|listener| listener.referenced)
        });
    if let Some(referenced) = referenced {
        request_socket_close(transport, id, referenced);
    } else {
        close_socket(transport, id);
    }
}
pub(crate) fn native_tcp_reference(transport: &mut Transport, id: u64, referenced: bool) {
    if let Some(socket) = transport.native_tcp_unbound.get_mut(&id) {
        socket.referenced = referenced;
    } else if let Some(connection) = transport.native_tcp_connecting.get_mut(&id) {
        connection.socket.referenced = referenced;
    } else if transport.listeners.contains_key(&id) {
        listener_reference(transport, id, referenced);
    } else {
        socket_reference(transport, id, referenced);
    }
}
pub(crate) fn native_tcp_has_ref(transport: &Transport, id: u64) -> bool {
    transport
        .native_tcp_unbound
        .get(&id)
        .map(|socket| socket.referenced)
        .or_else(|| {
            transport
                .native_tcp_connecting
                .get(&id)
                .map(|connection| connection.socket.referenced)
        })
        .or_else(|| listener_has_ref(transport, id))
        .or_else(|| socket_has_ref(transport, id))
        .unwrap_or(false)
}

/// The synchronous syscall frontier precedes request construction. Queued
/// bytes stay with the caller until init hooks have completed, so reentrant
/// native writes/close observe the same submission order as libuv.
pub(crate) enum NativeWriteAdmission {
    Synchronous,
    Queued { written: usize },
}
pub(crate) fn try_native_write(
    transport: &mut Transport,
    socket: u64,
    bytes: &[u8],
) -> std::io::Result<NativeWriteAdmission> {
    let stream = transport
        .sockets
        .get_mut(&socket)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))?;
    if !matches!(&stream.stream, SocketStream::Plain(_)) {
        return Err(std::io::Error::from_raw_os_error(libc::ENOTSUP));
    }
    if let Some(error) = &stream.write_failure {
        return Err(std::io::Error::from_raw_os_error(
            error.raw_os_error().unwrap_or(libc::EPIPE),
        ));
    }
    if stream.write_offset < stream.pending_write.len() {
        return Ok(NativeWriteAdmission::Queued { written: 0 });
    }
    if bytes.is_empty() {
        return Ok(NativeWriteAdmission::Synchronous);
    }
    let written = match stream.stream.write(bytes, 0) {
        Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
        Ok(written) => written,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
            ) =>
        {
            0
        }
        Err(error) => return Err(error),
    };
    Ok(if written == bytes.len() {
        NativeWriteAdmission::Synchronous
    } else {
        NativeWriteAdmission::Queued { written }
    })
}
/// Admit only the remaining real bytes after the request's init hooks. This
/// revalidates the stream because an init hook may close it or queue a write.
pub(crate) fn queue_native_write(
    transport: &mut Transport,
    socket: u64,
    request: RootId,
    bytes: &[u8],
) -> std::io::Result<()> {
    let stream = transport
        .sockets
        .get_mut(&socket)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))?;
    if let Some(error) = &stream.write_failure {
        return Err(std::io::Error::from_raw_os_error(
            error.raw_os_error().unwrap_or(libc::EPIPE),
        ));
    }
    stream.pending_write.extend_from_slice(bytes);
    stream.completions.push_back(WriteBoundary {
        end: stream.pending_write.len(),
        owner: WriteOwner::Native(request),
    });
    Ok(())
}

struct NativeConnecting {
    socket: super::tcp_owned::TcpOwned,
    request: RootId,
}
pub(crate) fn native_tcp_connect(
    transport: &mut Transport,
    id: u64,
    request: RootId,
    address: SocketAddr,
) -> std::io::Result<()> {
    if transport.native_tcp_connecting.contains_key(&id) {
        return Err(std::io::Error::from_raw_os_error(libc::EALREADY));
    }
    let socket = transport
        .native_tcp_unbound
        .get_mut(&id)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))?;
    socket.start_connect(address)?;
    let socket = transport
        .native_tcp_unbound
        .remove(&id)
        .expect("submitted native connect owns socket");
    transport
        .native_tcp_connecting
        .insert(id, NativeConnecting { socket, request });
    Ok(())
}
fn poll_native_connects(transport: &mut Transport) -> Vec<TransportEvent> {
    let ids = transport
        .native_tcp_connecting
        .keys()
        .copied()
        .collect::<Vec<_>>();
    let mut events = Vec::new();
    for id in ids {
        let result = transport.native_tcp_connecting[&id].socket.connect_ready();
        if matches!(result, Ok(false)) {
            continue;
        }
        let connection = transport
            .native_tcp_connecting
            .remove(&id)
            .expect("observed native connect");
        let request = connection.request;
        let status = match result {
            Ok(true) => {
                let referenced = connection.socket.referenced;
                let stream = SocketStream::Plain(RawStream::Tcp(connection.socket.into_stream()));
                transport
                    .sockets
                    .insert(id, SharedSocket::new(stream, ReadDemand::Idle, referenced));
                0
            }
            Err(error) => {
                // libuv keeps the actual failed TCP descriptor alive until close.
                transport.native_tcp_unbound.insert(id, connection.socket);
                -error.raw_os_error().unwrap_or(libc::EIO)
            }
            Ok(false) => unreachable!("pending connection skipped"),
        };
        events.push(TransportEvent::NativeConnectComplete { request, status });
    }
    events
}
pub(crate) fn native_shutdown_request(
    transport: &mut Transport,
    id: u64,
    request: RootId,
) -> std::io::Result<()> {
    let socket = transport
        .sockets
        .get_mut(&id)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::ENOTCONN))?;
    if socket.end_after_write || socket.write_ended {
        return Err(std::io::Error::from_raw_os_error(libc::ENOTCONN));
    }
    socket.shutdown_request = Some(request);
    socket.end_after_write = true;
    Ok(())
}

/// libuv prepends genuine close requests to its closing-handle list. Keep
/// lifecycle metadata in that same authoritative order until completion.
fn request_socket_close(transport: &mut Transport, id: u64, referenced: bool) {
    transport.closing_sockets.push_front((
        CloseTarget::Stream(id),
        ClosingSocket {
            referenced,
            phase: SocketClosePhase::Requested,
        },
    ));
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CloseTarget {
    Stream(u64),
    Datagram(u64),
}
pub(crate) fn request_udp_close(transport: &mut Transport, id: u64, referenced: bool) {
    transport.closing_sockets.push_front((
        CloseTarget::Datagram(id),
        ClosingSocket {
            referenced,
            phase: SocketClosePhase::Requested,
        },
    ));
}
pub(crate) fn udp_close_reference(transport: &mut Transport, id: u64, referenced: bool) {
    if let Some((_, owner)) = transport
        .closing_sockets
        .iter_mut()
        .find(|(key, _)| *key == CloseTarget::Datagram(id))
    {
        owner.referenced = referenced;
    }
}
pub(crate) fn udp_close_has_ref(transport: &Transport, id: u64) -> Option<bool> {
    transport
        .closing_sockets
        .iter()
        .find_map(|(key, owner)| (*key == CloseTarget::Datagram(id)).then_some(owner.referenced))
}

/// Project BoundSocket helpers from Node's net implementation.
pub(crate) fn bound_socket_projection(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let owner = context.host_function_data()?;
    let policy =
        crate::host::shared_vm::commonjs::node_library_module(context, "net.js", "net.js")?;
    for name in ["BoundSocket", "_TransferredBoundSocket"] {
        let key = context.string_rooted(name);
        let constructor = context.get_property_rooted(policy, key)?;
        context.define_data_property_rooted(owner, key, constructor, true, true, true)?;
    }
    Ok(context.undefined())
}

/// New kernel descriptor received over SCM_RIGHTS becomes this endpoint's sole
/// physical owner. The sender's endpoint/consumer metadata is never copied.
#[cfg(unix)]
pub(crate) fn native_tcp_receive(
    transport: &mut Transport,
    id: u64,
    fd: std::os::fd::OwnedFd,
) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let raw = fd.as_raw_fd();
    let mut accepts: libc::c_int = 0;
    let mut length = std::mem::size_of_val(&accepts) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            raw,
            libc::SOL_SOCKET,
            libc::SO_ACCEPTCONN,
            (&mut accepts as *mut libc::c_int).cast(),
            &mut length,
        )
    } < 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let socket = socket2::Socket::from(fd);
    socket.set_nonblocking(true)?;
    if socket.local_addr()?.as_socket().is_none() {
        return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
    }
    enum Received {
        Listener(TcpListener),
        Stream(std::net::TcpStream),
        Bound(super::tcp_owned::TcpOwned),
    }
    let received = if accepts != 0 {
        Received::Listener(socket.into())
    } else if socket.peer_addr().is_ok() {
        Received::Stream(socket.into())
    } else {
        Received::Bound(super::tcp_owned::TcpOwned::from_owned_descriptor(
            socket.into(),
        )?)
    };
    // Prepare all fallible ownership transitions before replacing the constructor
    // socket; import failure leaves the real shell closeable for retirement.
    transport
        .native_tcp_unbound
        .remove(&id)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EINVAL))?;
    match received {
        Received::Listener(listener) => {
            transport
                .listeners
                .insert(id, Listener::new(ListenerSocket::Tcp(listener)));
        }
        Received::Stream(socket) => {
            let mut stream = SharedSocket::new(
                SocketStream::Plain(RawStream::Tcp(socket)),
                ReadDemand::Idle,
                true,
            );
            stream.delivery = DeliveryPhase::Delivered;
            transport.sockets.insert(id, stream);
        }
        Received::Bound(socket) => {
            transport.native_tcp_unbound.insert(id, socket);
        }
    }
    Ok(())
}
pub(crate) fn listener_is_tcp(transport: &Transport, id: u64) -> bool {
    matches!(
        transport.listeners.get(&id).and_then(Listener::socket),
        Some(ListenerSocket::Tcp(_))
    )
}
