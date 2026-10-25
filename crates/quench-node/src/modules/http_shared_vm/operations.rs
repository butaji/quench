use crate::host::NodeHost;
use crate::modules::event_constructor_shared_vm::get;
use crate::modules::net_shared_vm;
use quench_runtime::{NativeArithmetic, NativeContext, RootId, RootedError, Value};
use std::io;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};

type Context<'a> = NativeContext<'a, NodeHost>;

const DEFAULT_LISTEN_HOST: &str = "127.0.0.1";

enum HttpListenAddress {
    Tcp(SocketAddr),
    #[cfg(unix)]
    Local(String),
}

const MODULE_FACTORY: &str = r#"((initializeServer, EventEmitter, requestFunction, createAgent, destroyAgent, connect, IncomingMessage, ServerResponse, kHighWaterMark, setupConnectionsTracking, kConnectionsCheckingInterval, connectionListener) => {
  const DEFAULT_HTTP_MAX_HEADER_SIZE = 16 * 1024;
  const DEFAULT_HTTP_HIGH_WATER_MARK = 64 * 1024;
  // Repeated Node options use the final command-line value. Keep this in sync
  // with startup parsing so the HTTP constant does not retain an earlier one.
  const maxHeaderArgument = process.execArgv.reduce((last, argument) =>
    argument.startsWith('--max-http-header-size=') ? argument : last, undefined);
  const HTTP_MAX_HEADER_SIZE = maxHeaderArgument === undefined
    ? DEFAULT_HTTP_MAX_HEADER_SIZE
    : Number(maxHeaderArgument.slice('--max-http-header-size='.length));
  const invalidArgument = (name, expected, value) => {
    let received;
    if (Array.isArray(value)) received = "an instance of Array";
    else if (value === null) received = "null";
    else if (typeof value === "string") received = `type string (${JSON.stringify(value)})`;
    else if (typeof value === "number" || typeof value === "boolean") {
      received = `type ${typeof value} (${String(value)})`;
    } else received = `type ${typeof value}`;
    const error = new TypeError(`The \"${name}\" argument must be ${expected}. Received ${received}`);
    error.code = "ERR_INVALID_ARG_TYPE";
    return error;
  };
  function Server(options, requestListener) {
      if (!(this instanceof Server)) return new Server(options, requestListener);
      EventEmitter.call(this);
      if (typeof options === "function") {
        requestListener = options;
        options = undefined;
      } else if (options !== undefined &&
          (options === null || typeof options !== "object" || Array.isArray(options))) {
        throw invalidArgument("options", "of type object", options);
      }
      if (requestListener !== undefined && typeof requestListener !== "function") {
        throw invalidArgument("requestListener", "of type function", requestListener);
      }
      options = options || {};
      if (options.insecureHTTPParser !== undefined &&
          typeof options.insecureHTTPParser !== 'boolean') {
        const value = options.insecureHTTPParser;
        const received = Array.isArray(value) ? 'an instance of Array'
          : value === null ? 'null'
            : typeof value === 'string' ? `type string ('${value}')`
              : typeof value === 'number' || typeof value === 'boolean'
                ? `type ${typeof value} (${String(value)})`
                : `an instance of ${value?.constructor?.name || 'Object'}`;
        const error = new TypeError(
          `The "options.insecureHTTPParser" property must be of type boolean. Received ${received}`
        );
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      if (options.httpValidation !== undefined) {
        const validation = options.httpValidation;
        if (!['strict', 'relaxed', 'insecure'].includes(validation)) {
          const received = typeof validation === 'string'
            ? `'${validation}'`
            : validation === null ? 'null' : String(validation);
          const error = new TypeError(
            `The property 'options.httpValidation' must be one of: 'strict', 'relaxed', 'insecure'. Received ${received}`
          );
          error.code = 'ERR_INVALID_ARG_VALUE';
          throw error;
        }
        if (options.insecureHTTPParser !== undefined) {
          const error = new TypeError(
            `The property 'options.httpValidation' cannot be used together with options.insecureHTTPParser. Received '${validation}'`
          );
          error.code = 'ERR_INVALID_ARG_VALUE';
          throw error;
        }
      }
      this.insecureHTTPParser = options.insecureHTTPParser;
      this.httpValidation = options.httpValidation;
      if (options.rejectNonStandardBodyWrites !== undefined &&
          typeof options.rejectNonStandardBodyWrites !== 'boolean') {
        throw invalidArgument(
          'options.rejectNonStandardBodyWrites',
          'of type boolean',
          options.rejectNonStandardBodyWrites,
        );
      }
      this.rejectNonStandardBodyWrites = options.rejectNonStandardBodyWrites ?? false;
      const validateTimeout = (name, value) => {
        if (value === undefined) return;
        if (typeof value !== "number" || !Number.isFinite(value)) {
          const error = new TypeError(`The "${name}" argument must be of type number. Received ${typeof value}`);
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        if (!Number.isInteger(value) || value < 0 || value > 2 ** 32 - 1) {
          const error = new RangeError(`The value of "${name}" is out of range.`);
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
      };
      validateTimeout("requestTimeout", options.requestTimeout);
      validateTimeout("headersTimeout", options.headersTimeout);
      validateTimeout("keepAliveTimeout", options.keepAliveTimeout);
      validateTimeout("keepAliveTimeoutBuffer", options.keepAliveTimeoutBuffer);
      validateTimeout("connectionsCheckingInterval", options.connectionsCheckingInterval);
      this.requestTimeout = options.requestTimeout === undefined ? 300000 : options.requestTimeout;
      this.headersTimeout = options.headersTimeout === undefined
        ? Math.min(60000, this.requestTimeout)
        : options.headersTimeout;
      if (this.requestTimeout > 0 && this.headersTimeout > 0 && this.headersTimeout > this.requestTimeout) {
        const error = new RangeError('The value of "headersTimeout" is out of range.');
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
      this.keepAliveTimeout = options.keepAliveTimeout === undefined ? 5000 : options.keepAliveTimeout;
      this.keepAliveTimeoutBuffer = options.keepAliveTimeoutBuffer === undefined ? 1000 : options.keepAliveTimeoutBuffer;
      this.connectionsCheckingInterval = options.connectionsCheckingInterval === undefined
        ? 30000
        : options.connectionsCheckingInterval;
      this.requireHostHeader = options.requireHostHeader === undefined ? true : options.requireHostHeader;
      this.joinDuplicateHeaders = options.joinDuplicateHeaders;
      this.uniqueHeaders = Array.isArray(options.uniqueHeaders)
        ? options.uniqueHeaders.map((name) => String(name).toLowerCase())
        : undefined;
      this.maxHeadersCount = null;
      this.maxRequestsPerSocket = options.maxRequestsPerSocket ?? 0;
      this.highWaterMark = options.highWaterMark ?? DEFAULT_HTTP_HIGH_WATER_MARK;
      this.maxHeaderSize = options.maxHeaderSize ?? HTTP_MAX_HEADER_SIZE;
      this._quenchIncomingMessage = options.IncomingMessage ?? IncomingMessage;
      this._quenchServerResponse = options.ServerResponse ?? ServerResponse;
      this.timeout = 0;
      this.on("listening", setupConnectionsTracking);
      this.on("close", () => {
        const interval = this[kConnectionsCheckingInterval];
        if (interval) clearInterval(interval);
      });
      initializeServer([this, requestListener]);
      // Accepted native sockets are parsed by Quench's shared HTTP poller.
      // For a caller-supplied Duplex (for example stream.duplexPair()), there
      // is no native HTTPParser handle to attach Node's connection listener
      // to, so parse the request head from the stream directly.
      this.on('connection', function onHttpConnection(socket) {
        if (socket && socket.__quenchHttpSocketId === undefined) {
          let buffered = Buffer.alloc(0);
          let handled = false;
          socket.on('data', (chunk) => {
            if (handled) return;
            buffered = Buffer.concat([buffered, Buffer.from(chunk)]);
            const headerEnd = buffered.indexOf('\r\n\r\n');
            if (headerEnd === -1) return;
            handled = true;
            const headerLength = headerEnd + 4;
            if (headerLength > (this.maxHeaderSize || HTTP_MAX_HEADER_SIZE)) {
              const error = new Error('Parse Error');
              error.code = 'HPE_HEADER_OVERFLOW';
              this.emit('clientError', error, socket);
              return;
            }
            const lines = buffered.subarray(0, headerEnd).toString('latin1').split('\r\n');
            const match = /^(\S+)\s+(\S+)\s+HTTP\/(\d+)\.(\d+)$/.exec(lines.shift() || '');
            if (!match) {
              const error = new Error('Parse Error');
              error.code = 'HPE_INVALID_CONSTANT';
              this.emit('clientError', error, socket);
              return;
            }
            const request = new this._quenchIncomingMessage(socket);
            request.method = match[1];
            request.url = match[2];
            request.httpVersionMajor = Number(match[3]);
            request.httpVersionMinor = Number(match[4]);
            request.httpVersion = `${match[3]}.${match[4]}`;
            request.rawHeaders = [];
            for (const line of lines) {
              const colon = line.indexOf(':');
              if (colon > 0) request.rawHeaders.push(line.slice(0, colon).trim(), line.slice(colon + 1).trim());
            }
            request._addHeaderLines(request.rawHeaders, request.rawHeaders.length);
            request.complete = true;
            // ServerResponse's native lifecycle is keyed by a Quench HTTP
            // exchange id. Manually supplied Duplex streams have no such
            // exchange, so provide the small response surface used by this
            // request path without entering the host-backed response writer.
            const response = new EventEmitter();
            const responseHeaders = new Map();
            response.statusCode = 200;
            response.statusMessage = undefined;
            response.headersSent = false;
            response.writableEnded = false;
            response.finished = false;
            response.setHeader = (name, value) => {
              responseHeaders.set(String(name).toLowerCase(), value);
              return response;
            };
            response.getHeader = (name) => responseHeaders.get(String(name).toLowerCase());
            response.hasHeader = (name) => responseHeaders.has(String(name).toLowerCase());
            response.getHeaders = () => Object.fromEntries(responseHeaders);
            response.getHeaderNames = () => Array.from(responseHeaders.keys());
            response.removeHeader = (name) => responseHeaders.delete(String(name).toLowerCase());
            response.writeHead = (statusCode, statusMessage, headers) => {
              response.statusCode = statusCode;
              if (statusMessage && typeof statusMessage === 'object') headers = statusMessage;
              else if (typeof statusMessage === 'string') response.statusMessage = statusMessage;
              if (headers) for (const [name, value] of Object.entries(headers)) response.setHeader(name, value);
              response.headersSent = true;
              return response;
            };
            response.write = () => {
              response.headersSent = true;
              return true;
            };
            response.end = (chunk, encoding, callback) => {
              const done = typeof chunk === 'function' ? chunk
                : typeof encoding === 'function' ? encoding : callback;
              response.headersSent = true;
              response.writableEnded = true;
              response.finished = true;
              response.emit('finish');
              if (done) done();
              return response;
            };
            this.emit('request', request, response);
          });
        }
      });
  }
  Object.setPrototypeOf(Server, EventEmitter);
  Object.setPrototypeOf(Server.prototype, EventEmitter.prototype);
  if (typeof Symbol.asyncDispose === "symbol") {
    Object.defineProperty(Server.prototype, Symbol.asyncDispose, {
      configurable: true,
      value: function asyncDispose() {
        return new Promise((resolve, reject) => {
          this.close((error) => error ? reject(error) : resolve());
        });
      },
    });
  }
  Object.defineProperty(Server.prototype, "setTimeout", {
    configurable: true, writable: true,
    value: function setTimeout(msecs, callback) {
      this.timeout = msecs;
      if (callback !== undefined) this.on("timeout", callback);
      return this;
    }
  });
  const createServer = (...args) => new Server(...args);
  const AgentClass = class Agent {
    constructor(options) {
      EventEmitter.call(this);
      options = options || {};
      for (const [name, value] of [
        ["maxSockets", options.maxSockets],
        ["maxTotalSockets", options.maxTotalSockets],
      ]) {
        if (value === undefined || value === Infinity) continue;
        if (typeof value !== "number") {
          const received = typeof value === "string"
            ? `type string ('${value}')`
            : typeof value;
          const error = new TypeError(
            `The "${name}" argument must be of type number. Received ${received}`
          );
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        if (Number.isNaN(value) || value <= 0) {
          const error = new RangeError(`The value of "${name}" is out of range.`);
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
      }
      this._quenchSharedAgentId = createAgent(options);
      this.options = options;
      this.timeout = options.timeout ?? 0;
      this.defaultPort = 80;
      this.protocol = 'http:';
      this.maxSockets = options.maxSockets ?? Agent.defaultMaxSockets;
      this.maxFreeSockets = options.maxFreeSockets ?? 256;
      this.maxTotalSockets = options.maxTotalSockets ?? Infinity;
      this.keepAlive = options.keepAlive ?? false;
      this.keepAliveMsecs = options.keepAliveMsecs ?? 1000;
      this.scheduling = options.scheduling ?? 'lifo';
      const keepAliveBuffer = options.agentKeepAliveTimeoutBuffer;
      this.agentKeepAliveTimeoutBuffer = Number.isFinite(keepAliveBuffer) && keepAliveBuffer >= 0
        ? keepAliveBuffer
        : 1000;
      this.sockets = Object.create(null);
      this.freeSockets = Object.create(null);
      this.requests = Object.create(null);
    }
    get totalSocketCount() {
      let total = 0;
      for (const sockets of Object.values(this.sockets)) total += sockets.length;
      for (const sockets of Object.values(this.freeSockets)) total += sockets.length;
      return total;
    }
    getName(options) {
      const host = options.host || options.hostname || 'localhost';
      const port = options.port || '';
      let name = `${host}:${port}:${options.localAddress || ''}`;
      if (options.family === 4 || options.family === 6) name += `:${options.family}`;
      if (options.socketPath) name += `:${options.socketPath}`;
      return name;
    }
    addRequest(request, options, port, localAddress) {
      if (typeof options === 'string') {
        options = { host: options, port: port || this.defaultPort, localAddress };
      } else {
        options = options || {};
        options = {
          host: options.host ?? request.host,
          hostname: options.hostname ?? request.host,
          port: options.port ?? request.port,
          localAddress: options.localAddress ?? request.localAddress,
          family: options.family,
          socketPath: options.socketPath,
        };
      }
      const name = this.getName(options);
      const sockets = this.sockets[name] || (this.sockets[name] = []);
      const freeSockets = this.freeSockets[name] || [];
      if (freeSockets.length > 0) {
        const socket = freeSockets.pop();
        if (freeSockets.length === 0) delete this.freeSockets[name];
        request.onSocket(socket);
        return;
      }
      if (sockets.length < this.maxSockets && sockets.length < this.maxTotalSockets) {
        const socket = this.createConnection(options);
        sockets.push(socket);
        request.onSocket(socket);
        return;
      }
      (this.requests[name] || (this.requests[name] = [])).push(request);
    }
    createConnection(options = {}, callback) {
      if (typeof options === 'function') { callback = options; options = {}; }
      const signal = options && options.signal;
      const connectionOptions = { ...options };
      delete connectionOptions.signal;
      const socket = connect(connectionOptions, callback);
      if (socket && this._quenchKeylogForwarder) {
        socket.on('keylog', this._quenchKeylogForwarder);
      }
      if (signal && typeof signal.addEventListener === 'function') {
        const abort = () => {
          const error = new Error('The operation was aborted');
          error.name = 'AbortError';
          error.code = 'ABORT_ERR';
          socket.destroy(error);
        };
        const removeAbortListener = () => signal.removeEventListener('abort', abort);
        if (signal.aborted) queueMicrotask(abort);
        else {
          signal.addEventListener('abort', abort, { once: true });
          socket.once('close', removeAbortListener);
        }
      }
      return socket;
    }
    createSocket(request, options = {}, callback) {
      // ClientRequest passes its original HTTP options here, where `path` is
      // the request target (for example `/0`), not a Unix socket path. net.connect
      // interprets `path` as a local socket endpoint, so strip the HTTP target
      // and translate the documented `socketPath` option explicitly.
      const connectionOptions = { ...options, ...this.options };
      if (connectionOptions.socketPath !== undefined) {
        connectionOptions.path = connectionOptions.socketPath;
      } else {
        delete connectionOptions.path;
      }
      // Agent.createSocket normally receives addRequest's normalized options.
      // This implementation is called directly from ClientRequest, so fill
      // the endpoint from the request when callers only pass lookup/timeout.
      if (connectionOptions.path === undefined && connectionOptions.socketPath === undefined) {
        connectionOptions.port ??= request.port ?? this.defaultPort;
        // URL-parsed request options can have `host` as an authority string
        // including `:port`; Agent connection setup needs the normalized
        // hostname separately.
        connectionOptions.host = request.host ?? connectionOptions.hostname ?? connectionOptions.host ?? 'localhost';
      }
      if (this.keepAlive) {
        connectionOptions.keepAlive = true;
        connectionOptions.keepAliveInitialDelay = this.keepAliveMsecs;
      }
      const timeout = request.timeout || this.options.timeout || 0;
      if (timeout) connectionOptions.timeout = timeout;
      // `createConnection` is commonly overridden with the Node callback
      // contract. Keep that contract intact even when a caller reaches
      // createSocket without supplying its own completion callback: otherwise
      // overrides that defer `cb(error)` can throw from nextTick instead of
      // delivering the failure to the request.
      let completed = false;
      const oncreate = (error, socket) => {
        // Node wraps this callback in `once()`: custom createConnection
        // implementations may both return a socket and invoke the callback.
        if (completed) return;
        completed = true;
        if (typeof callback === 'function') return callback(error, socket);
        if (error) request.onSocket(socket, error);
        else if (socket) request.onSocket(socket);
      };
      const socket = this.createConnection(connectionOptions, oncreate);
      if (socket && typeof socket.on === 'function') {
        const agent = this;
        const name = this.getName({
          ...connectionOptions,
          host: request.host ?? connectionOptions.host,
          port: request.port ?? connectionOptions.port,
        });
        const removeFromPools = () => {
          for (const pool of [agent.sockets[name], agent.freeSockets[name]]) {
            if (!pool) continue;
            const index = pool.indexOf(socket);
            if (index !== -1) pool.splice(index, 1);
          }
          if (agent.sockets[name]?.length === 0) delete agent.sockets[name];
          if (agent.freeSockets[name]?.length === 0) delete agent.freeSockets[name];
        };
        // Node installs the free-socket error guard only while the socket is
        // idle in freeSockets. Active request errors are owned by the
        // ClientRequest listener until the response releases the socket.
        socket.__quenchFreeSocketErrorListener = () => {
          if (agent.freeSockets[name]?.includes(socket)) socket.destroy();
        };
        this.trackSocketEnd(socket, name, removeFromPools);
        // Node removes an agent-owned socket from its pools immediately when
        // `agentRemove` is emitted (for upgrades and other socket handoffs).
        socket.on('agentRemove', removeFromPools);
        socket.once('close', removeFromPools);
      }
      if (timeout) {
        // An idle keep-alive socket that times out must be evicted before the
        // ClientRequest timeout listener runs, matching Node's Agent ordering.
        const agent = this;
        const name = this.getName({
          ...connectionOptions,
          host: connectionOptions.host ?? request.host,
          port: connectionOptions.port ?? request.port,
        });
        socket.on('timeout', function onTimeout() {
          const free = agent.freeSockets[name];
          const index = free?.indexOf(socket) ?? -1;
          if (index === -1) return;
          free.splice(index, 1);
          if (free.length === 0) delete agent.freeSockets[name];
          socket.destroy();
        });
        socket.setTimeout(timeout);
        request.timeoutCb ||= function emitRequestTimeout() {
          request.emit('timeout');
        };
        socket.once('timeout', request.timeoutCb);
      }
      // `createConnection` may complete asynchronously. Do not synthesize a
      // successful callback with an undefined socket, since that would
      // consume Node's one-shot callback before the real error arrives.
      if (socket) oncreate(null, socket);
      return socket;
    }
    trackSocketEnd(socket, name, removeFromPools) {
      const onSocketEnd = () => {
        if (this.freeSockets[name]?.includes(socket)) removeFromPools();
      };
      socket.on('end', onSocketEnd);
      // Node removes its Agent end guard while the socket is idle and adds it
      // again when the socket is assigned to a subsequent request.
      socket.once('free', () => socket.removeListener('end', onSocketEnd));
    }
    keepSocketAlive(socket) {
      if (!socket || socket.destroyed) return false;
      if (this.keepAliveMsecs > 0) socket.setKeepAlive?.(true, this.keepAliveMsecs);
      socket.unref?.();
      return true;
    }
    destroy() {
      return destroyAgent(this._quenchSharedAgentId);
    }
  };
  const agentOn = EventEmitter.prototype.on;
  AgentClass.prototype.on = function(event, listener) {
    const result = agentOn.call(this, event, listener);
    if (event === 'keylog' && typeof listener === 'function') {
      if (!this._quenchKeylogForwarder) {
        this._quenchKeylogForwarder = (...args) => this.emit('keylog', ...args);
      }
      for (const pools of [this.freeSockets, this.sockets]) {
        for (const sockets of Object.values(pools)) {
          for (const socket of sockets) {
            if (!socket.listenerCount('keylog')) {
              socket.on('keylog', this._quenchKeylogForwarder);
            }
          }
        }
      }
    }
    return result;
  };
  function Agent(options) {
    if (!(this instanceof Agent)) return new Agent(options);
    return Reflect.construct(AgentClass, [options], new.target);
  }
  Object.setPrototypeOf(Agent.prototype, AgentClass.prototype);
  Object.setPrototypeOf(AgentClass.prototype, EventEmitter.prototype);
  Object.setPrototypeOf(AgentClass, EventEmitter);
  Object.setPrototypeOf(Agent, AgentClass);
  Agent.defaultMaxSockets = 15;
  const globalAgent = new Agent();
  globalAgent.protocol = 'http:';
  function request(input, options, callback) {
    if (typeof options === 'function') { callback = options; options = undefined; }
    if (input && typeof input === 'object') {
      input = {
        ...input,
        ...(typeof input.href === 'string' ? { href: input.href } : {}),
        agent: input.agent ?? globalAgent,
      };
    } else {
      options = { ...(options || {}), agent: options?.agent ?? globalAgent };
    }
    const outgoing = requestFunction(input, options, callback);
    outgoing.agent = input && typeof input === 'object' ? input.agent : options.agent;
    const requestOptions = input && typeof input === 'object' ? input : options;
    outgoing[kHighWaterMark] = requestOptions?.highWaterMark ?? DEFAULT_HTTP_HIGH_WATER_MARK;
    return outgoing;
  }
  // The low-level request factory owns the Writable-backed prototype used by
  // each request. Reuse it here so requests returned through this public API
  // also satisfy `instanceof http.ClientRequest`.
  request.prototype = requestFunction.prototype;
  Object.defineProperty(request.prototype, 'constructor', {
    configurable: true, writable: true, value: request,
  });
  const get = (input, options, callback) => {
    const outgoing = request(input, options, callback);
    outgoing.end();
    return outgoing;
  };
  function websocketMask(source, mask, output, offset, length) {
    const invalidType = (name) => {
      const error = new TypeError(`The "${name}" argument must be an ArrayBufferView`);
      error.code = 'ERR_INVALID_ARG_TYPE';
      return error;
    };
    if (!ArrayBuffer.isView(source)) throw invalidType('source');
    if (!ArrayBuffer.isView(mask)) throw invalidType('mask');
    if (mask.byteLength !== 4) {
      const error = new TypeError('The "mask" argument must be 4 bytes long');
      error.code = 'ERR_INVALID_ARG_VALUE';
      throw error;
    }
    if (!ArrayBuffer.isView(output)) throw invalidType('output');
    if (offset === undefined || length === undefined) {
      const error = new TypeError('The "offset" and "length" arguments are required');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (typeof offset !== 'number' || typeof length !== 'number') {
      const error = new TypeError('The "offset" and "length" arguments must be numbers');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (!Number.isInteger(offset) || offset < 0 || offset > output.byteLength || !Number.isFinite(offset)) {
      const error = new RangeError('The value of "offset" is out of range.');
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    if (!Number.isInteger(length) || length < 0 || length > source.byteLength || length > output.byteLength - offset) {
      const error = new RangeError('The value of "length" is out of range.');
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    if (output.buffer.immutable) {
      const error = new TypeError("The 'output' must not be backed by an immutable ArrayBuffer");
      error.code = 'ERR_INVALID_ARG_VALUE';
      throw error;
    }
    if (length === 0) return;
    const bytes = new Uint8Array(source.buffer, source.byteOffset, source.byteLength).slice(0, length);
    const key = new Uint8Array(mask.buffer, mask.byteOffset, mask.byteLength).slice(0, 4);
    const target = new Uint8Array(output.buffer, output.byteOffset, output.byteLength);
    for (let index = 0; index < length; index++)
      target[offset + index] = bytes[index] ^ key[index & 3];
  }
  function websocketUnmask(buffer, mask) {
    if (!ArrayBuffer.isView(buffer) || !ArrayBuffer.isView(mask)) {
      const error = new TypeError('buffer and key must be ArrayBufferView instances');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (mask.byteLength !== 4) {
      const error = new TypeError('The "mask" argument must be 4 bytes long');
      error.code = 'ERR_INVALID_ARG_VALUE';
      throw error;
    }
    if (buffer.buffer.immutable) {
      const error = new TypeError("The 'buffer' must not be backed by an immutable ArrayBuffer");
      error.code = 'ERR_INVALID_ARG_VALUE';
      throw error;
    }
    if (buffer.byteLength === 0) return;
    const bytes = new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
    const key = new Uint8Array(mask.buffer, mask.byteOffset, mask.byteLength).slice(0, 4);
    const length = buffer.byteLength;
    for (let index = 0; index < length; index++)
      bytes[index] ^= key[index & 3];
  }
  const httpToken = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
  const invalidHeaderValue = /[^\t\x20-\x7e\x80-\xff]/;
  const invalidRelaxedHeaderValue = /[\x00\x0a\x0d]|[^\x00-\xff]/;
  function isValidHeaderName(name) {
    return typeof name === 'string' && name.length > 0 && httpToken.test(name);
  }
  function isValidHeaderValue(value, options) {
    let relaxed = false;
    if (options !== undefined) {
      if (options === null || typeof options !== 'object' || Array.isArray(options))
        throw invalidArgument('options', 'of type object', options);
      const validation = options.httpValidation;
      if (validation === 'relaxed') relaxed = true;
      else if (validation !== undefined && validation !== 'strict') {
        const received = typeof validation === 'string'
          ? ` Received '${validation}'`
          : validation === null ? ' Received null'
            : ` Received ${String(validation)}`;
        const error = new TypeError(`The property 'options.httpValidation' must be one of: 'strict', 'relaxed'.${received}`);
        error.code = 'ERR_INVALID_ARG_VALUE';
        throw error;
      }
    }
    if (value === undefined || typeof value === 'symbol') return false;
    return !(relaxed ? invalidRelaxedHeaderValue : invalidHeaderValue).test(value);
  }
  function validateHeaderName(name) {
    if (isValidHeaderName(name)) return;
    const error = new TypeError(`Header name must be a valid HTTP token [${String(name)}]`);
    error.code = 'ERR_INVALID_HTTP_TOKEN';
    throw error;
  }
  function validateHeaderValue(name, value) {
    if (value === undefined) {
      const error = new TypeError(`The "${name}" header content must not be undefined`);
      error.code = 'ERR_HTTP_INVALID_HEADER_VALUE';
      throw error;
    }
    if (invalidHeaderValue.test(value)) {
      const error = new TypeError(`Invalid character in header content ["${name}"]`);
      error.code = 'ERR_INVALID_CHAR';
      throw error;
    }
  }
  return { Server, createServer, request, get, Agent, globalAgent, IncomingMessage, ServerResponse, websocketMask, websocketUnmask, isValidHeaderName, isValidHeaderValue, validateHeaderName, validateHeaderValue, maxHeaderSize: HTTP_MAX_HEADER_SIZE };
})"#;

const SERVER_CLOSE_FACTORY: &str = r#"((closeOperation) => function(callback) {
  if (callback !== undefined) this.once("close", callback);
  closeOperation.call(this);
  return this;
})"#;

const RESPONSE_FACTORY: &str = r#"((Writable, OutgoingMessage, setHeaderOperation, getHeaderOperation, removeHeader, write, finish, writeHeadOperation, destroyOperation, initializeResponse, kHighWaterMark, publishDiagnostics) => {
  const DEFAULT_HTTP_HIGH_WATER_MARK = 64 * 1024;
  const destroyWritable = Writable.prototype.destroy;
  const invalidInformationHeaderValue = /[^\t\x20-\x7e\x80-\xff]/;
  const writableLengthGetter = Object.getOwnPropertyDescriptor(Writable.prototype, 'writableLength')?.get;
  const writableErroredGetter = Object.getOwnPropertyDescriptor(Writable.prototype, 'errored')?.get;
  const contentLengthMismatch = (actual, expected) => {
    const error = new Error(
      `Response body's content-length of ${actual} byte(s) does not match the content-length of ${expected} byte(s) set in header`
    );
    error.code = 'ERR_HTTP_CONTENT_LENGTH_MISMATCH';
    return error;
  };
  const headersByResponse = new WeakMap();
  const headers = (response) => {
    let value = headersByResponse.get(response);
    if (value === undefined) {
      value = new Map();
      headersByResponse.set(response, value);
    }
    return value;
  };
  function ServerResponse(id, statusCode, request, server, socket) {
    if (typeof id !== "number") {
      socket = undefined;
      server = undefined;
      request = id;
      id = 0;
      statusCode = 200;
    }
    let response;
    let ending = false;
    let pendingResponseWrites = [];
    let responseWriteFlushScheduled = false;
    const bodyNotAllowed = () => request?.method === 'HEAD' ||
      response.statusCode === 204 || response.statusCode === 304 ||
      (response.statusCode >= 100 && response.statusCode < 200);
    const throwBodyNotAllowed = () => {
      const error = new Error('Adding content for this request method or response status is not allowed.');
      error.code = 'ERR_HTTP_BODY_NOT_ALLOWED';
      throw error;
    };
    const flushResponseWrites = () => {
      if (pendingResponseWrites.length === 0) {
        responseWriteFlushScheduled = false;
        return;
      }
      responseWriteFlushScheduled = false;
      const writes = pendingResponseWrites;
      pendingResponseWrites = [];
      for (let index = 0; index < writes.length;) {
        const first = index;
        const explicit = writes[index].explicit;
        const chunks = [];
        while (index < writes.length && writes[index].explicit === explicit) {
          chunks.push(writes[index++].chunk);
        }
        try {
          write(id, Buffer.concat(chunks), explicit, response.statusCode,
            response.statusMessage, response, undefined);
        } catch (error) {
          for (let callbackIndex = first; callbackIndex < index; callbackIndex++) {
            writes[callbackIndex].callback(error);
          }
          continue;
        }
        for (let callbackIndex = first; callbackIndex < index; callbackIndex++) {
          writes[callbackIndex].callback();
        }
      }
    };
    const normalizeHeaderValue = (value) => {
      if (value === undefined) return [value, false];
      if (Array.isArray(value)) return [value.map((item) => String(item)), true];
      return [String(value), false];
    };
    const uniqueHeaderNames = new Set(Array.isArray(server?.uniqueHeaders)
      ? server.uniqueHeaders.map((name) => String(name).toLowerCase())
      : []);
    response = new Writable({
      // Writable `finish` completes the response body, while the socket owns
      // the response's transport lifetime.
      highWaterMark: server?.highWaterMark ?? DEFAULT_HTTP_HIGH_WATER_MARK,
      autoDestroy: false,
      write(chunk, encoding, callback) {
        if (id === 0 && response.socket && typeof response.socket.write === 'function') {
          try { response.socket.write(chunk, encoding, callback); }
          catch (error) { callback(error); }
          return;
        }
        // Buffer writes completed in the same turn so chunked output can frame
        // their combined payload, matching OutgoingMessage's socket corking.
        const bytes = typeof chunk === 'string'
          ? Buffer.from(chunk, encoding || 'utf8')
          : Buffer.from(chunk);
        pendingResponseWrites.push({ chunk: bytes, explicit: !ending, callback });
        response.headersSent = true;
        if (!responseWriteFlushScheduled) {
          responseWriteFlushScheduled = true;
          process.nextTick(flushResponseWrites);
        }
      },
      final(callback) {
        try {
          flushResponseWrites();
          finish(id, response.statusCode, response.statusMessage, request, response, server, socket);
          response.headersSent = true;
          const attachedSocket = response.socket;
          if (attachedSocket && typeof response.detachSocket === 'function') {
            response.detachSocket(attachedSocket);
          }
          // Node closes an OutgoingMessage after detaching it from the socket;
          // emit close before Writable's final callback so finished() observes
          // the same terminal order for ServerResponse instances.
          if (!response._closed) {
            response.destroyed = true;
            response._closed = true;
            response.emit('close');
          }
          callback();
        } catch (error) { callback(error); }
      },
      destroy(error, callback) {
        try { destroyOperation(id); callback(error); }
        catch (failure) { callback(failure); }
      },
    });
    // Preserve Writable's end implementation before installing the Node
    // ServerResponse -> OutgoingMessage prototype chain below. Otherwise the
    // captured method resolves to OutgoingMessage.end and a second end call
    // raises ERR_STREAM_WRITE_AFTER_END.
    const endStream = response.end.bind(response);
    // Capture Writable.write for the same reason. Once the HTTP prototype
    // chain is installed, response.write resolves to OutgoingMessage.write,
    // which writes through the socket itself and bypasses the host response
    // lifecycle used by this wrapper.
    const writeStream = response.write.bind(response);
    // Construct the private OutgoingMessage slot state on this Writable
    // instance so its Node prototype methods (notably destroy/uncork) can be
    // used by ServerResponse. The stream's public write/end methods remain
    // captured above because this response's transport is host driven.
    OutgoingMessage.call(response);
    response[kHighWaterMark] = server?.highWaterMark ?? DEFAULT_HTTP_HIGH_WATER_MARK;
    Object.setPrototypeOf(response, new.target?.prototype ?? ServerResponse.prototype);
    // Node's OutgoingMessage keeps this legacy public flag true after end();
    // the underlying Writable state still rejects subsequent writes.
    Object.defineProperty(response, 'writable', {
      configurable: true,
      enumerable: true,
      writable: true,
      value: true,
    });
    response.req = request;
    response.uniqueHeaders = Array.from(uniqueHeaderNames);
    // OutgoingMessage normally reports headersSent from its internal _header
    // field. This wrapper sends headers through the host operation instead,
    // so keep the state explicitly on the response object.
    Object.defineProperty(response, 'headersSent', {
      configurable: true,
      value: false,
      writable: true,
    });
    // ServerResponse is assembled around a Writable instance rather than
    // constructed through OutgoingMessage, so initialize the public
    // OutgoingMessage state that its constructor would normally install.
    response.destroyed = false;
    response._closed = false;
    response.strictContentLength = false;
    // Outgoing header validation follows the Server's configured parser mode.
    response._httpValidation = server?.httpValidation;
    Object.defineProperty(response, 'writableCorked', {
      configurable: true,
      value: 0,
      writable: true,
    });
    if (socket !== undefined) response.assignSocket(socket);
    if (request !== undefined && server !== undefined && socket !== undefined) {
      publishDiagnostics('http.server.response.created', { request, response, server, socket });
    }
    if (writableErroredGetter) {
      Object.defineProperty(response, 'errored', {
        configurable: true,
        get() {
          const error = writableErroredGetter.call(response);
          return error === null ? undefined : error;
        },
      });
    }
    let chunkFramingLength = 0;
    let pendingChunkBytes = 0;
    let strictContentLengthBytes = 0;
    if (writableLengthGetter) {
      Object.defineProperty(response, 'writableLength', {
        configurable: true,
        get() { return writableLengthGetter.call(response) + pendingChunkBytes + chunkFramingLength; },
      });
    }
    const writeChunk = writeStream;
    response.write = (chunk, encoding, callback) => {
        if (typeof encoding === 'function') { callback = encoding; encoding = undefined; }
        if (typeof encoding === 'string' && !Buffer.isEncoding(encoding)) {
          const error = new TypeError(`Unknown encoding: ${encoding}`);
          error.code = 'ERR_UNKNOWN_ENCODING';
          throw error;
        }
        if (server?.rejectNonStandardBodyWrites && bodyNotAllowed()) {
          throwBodyNotAllowed();
        }
        const bytes = typeof chunk === 'string'
          ? Buffer.byteLength(chunk, encoding || 'utf8')
          : chunk?.byteLength ?? 0;
        const expectedHeader = response.getHeader('content-length');
        const expectedContentLength = expectedHeader === undefined ? undefined : Number(expectedHeader);
        if (response.strictContentLength && expectedContentLength !== undefined &&
            Number.isFinite(expectedContentLength) &&
            strictContentLengthBytes + bytes > expectedContentLength) {
          throw contentLengthMismatch(strictContentLengthBytes + bytes, expectedContentLength);
        }
        strictContentLengthBytes += bytes;
        const framing = bytes === 0 ? 0 : bytes.toString(16).length + 4;
        chunkFramingLength += framing;
        pendingChunkBytes += bytes;
        return writeChunk(chunk, encoding, (error) => {
          chunkFramingLength -= framing;
          pendingChunkBytes -= bytes;
          if (callback) callback(error);
        });
    };
    response.finished = false;
    response.statusCode = statusCode;
    response.end = (chunk, encoding, callback) => {
      if (server?.rejectNonStandardBodyWrites && chunk && bodyNotAllowed()) {
        throwBodyNotAllowed();
      }
      let finalBytes = 0;
      if (chunk !== undefined && typeof chunk !== 'function') {
        finalBytes = typeof chunk === 'string'
          ? Buffer.byteLength(chunk, typeof encoding === 'string' ? encoding : 'utf8')
          : chunk?.byteLength ?? 0;
      }
      const expectedHeader = response.getHeader('content-length');
      const expectedContentLength = expectedHeader === undefined ? undefined : Number(expectedHeader);
      if (response.strictContentLength && expectedContentLength !== undefined &&
          Number.isFinite(expectedContentLength) &&
          strictContentLengthBytes + finalBytes !== expectedContentLength) {
        throw contentLengthMismatch(strictContentLengthBytes + finalBytes, expectedContentLength);
      }
      ending = true;
      response.finished = true;
      return endStream(chunk, encoding, callback);
    };
    response.setHeader = (name, value) => {
      if (response.headersSent) {
        const error = new Error('Cannot set headers after they are sent to the client');
        error.code = 'ERR_HTTP_HEADERS_SENT';
        throw error;
      }
      const lower = String(name).toLowerCase();
      const [normalized, multiple] = normalizeHeaderValue(value);
      setHeaderOperation(id, name, normalized, multiple,
        response._httpValidation === 'relaxed' || response._httpValidation === 'insecure');
      if (uniqueHeaderNames.has(lower) && Array.isArray(normalized)) {
        setHeaderOperation(id, name, normalized.join('; '), false,
          response._httpValidation === 'relaxed' || response._httpValidation === 'insecure');
      }
      headers(response).set(lower, { name: String(name), value: Array.isArray(normalized) ? normalized.slice() : normalized });
      return response;
    };
    response.getHeader = (name) => {
      const entry = headers(response).get(String(name).toLowerCase());
      if (entry !== undefined) return Array.isArray(entry.value) ? entry.value.slice() : entry.value;
      return getHeaderOperation(id, name);
    };
    response.getHeaders = () => {
      const result = Object.create(null);
      for (const [name, entry] of headers(response))
        result[name] = Array.isArray(entry.value) ? entry.value.slice() : entry.value;
      return result;
    };
    response.getHeaderNames = () => Array.from(headers(response).keys());
    response.getRawHeaderNames = () => Array.from(headers(response).values(), (entry) => entry.name);
    response.hasHeader = (name) => headers(response).has(String(name).toLowerCase());
    response.removeHeader = (name) => {
      if (response.headersSent) {
        const error = new Error('Cannot remove headers after they are sent to the client');
        error.code = 'ERR_HTTP_HEADERS_SENT';
        throw error;
      }
      headers(response).delete(String(name).toLowerCase());
      removeHeader(id, name);
      return response;
    };
    response.appendHeader = (name, value) => {
      const key = String(name).toLowerCase();
      const previous = headers(response).get(key);
      if (previous === undefined) return response.setHeader(name, value);
      const prior = Array.isArray(previous.value) ? previous.value : [previous.value];
      const values = prior.concat(Array.isArray(value) ? value : [value]);
      // Node retains the spelling from the first setHeader() call when later
      // appendHeader() calls use a different case variant.
      // setHeader() preserves the public array and applies the configured
      // unique-header wire separator while updating native response state.
      return response.setHeader(previous.name, values);
    };
    response.setHeaders = (values) => {
      if (response.headersSent) {
        const error = new Error('Cannot set headers after they are sent to the client');
        error.code = 'ERR_HTTP_HEADERS_SENT';
        throw error;
      }
      const isMap = values instanceof Map;
      const isHeaders = typeof Headers === 'function' && values instanceof Headers;
      if (!isMap && !isHeaders) {
        const error = new TypeError('The "headers" argument must be an instance of Map');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      if (isMap) {
        for (const [name, value] of values) response.setHeader(name, value);
      } else {
        const cookies = typeof values.getSetCookie === 'function' ? values.getSetCookie() : undefined;
        values.forEach((value, name) => {
          if (name !== 'set-cookie' || cookies === undefined) response.setHeader(name, value);
        });
        if (cookies !== undefined && cookies.length > 0) response.setHeader('set-cookie', cookies);
      }
      return response;
    };
    response.writeHead = (status, reasonOrHeaders, headersArg) => {
      const hasReason = typeof reasonOrHeaders === "string";
      const reason = hasReason ? reasonOrHeaders : undefined;
      // The three-argument overload permits an omitted status message, e.g.
      // writeHead(statusCode, undefined, headers). Preserve its third
      // argument instead of treating the missing second argument as the
      // complete header source.
      const source = hasReason || reasonOrHeaders === undefined && headersArg !== undefined
        ? headersArg
        : reasonOrHeaders;
      const entries = [];
      let invalid = false;
      if (source !== undefined) {
        if (Array.isArray(source)) {
          if (source.length % 2 !== 0) invalid = true;
          else {
            for (let index = 0; index < source.length; index += 2) {
              const [value, multiple] = normalizeHeaderValue(source[index + 1]);
              entries.push(source[index], value, multiple);
            }
          }
        } else if (source !== null && typeof source === "object") {
          for (const name of Object.keys(source)) {
            const [value, multiple] = normalizeHeaderValue(source[name]);
            entries.push(name, value, multiple);
          }
        } else {
          invalid = true;
        }
      }
      // Node advertises the configured idle keep-alive timeout on HTTP/1.1
      // responses. Keep the value in the final header list so it is visible
      // both on the wire and through IncomingMessage.headers on the client.
      const keepAliveTimeout = Number(server?.keepAliveTimeout);
      const explicitConnection = entries.some((name, index) => index % 3 === 0 &&
        String(name).toLowerCase() === 'connection') || response.hasHeader('connection');
      const closesConnection = request?.headers?.connection?.toLowerCase?.() === 'close' ||
        entries.some((name, index) => index % 3 === 0 &&
          String(name).toLowerCase() === 'connection' &&
          String(entries[index + 1]).toLowerCase() === 'close') ||
        String(response.getHeader('connection') || '').toLowerCase() === 'close';
      const hasKeepAlive = entries.some((name, index) => index % 3 === 0 &&
        String(name).toLowerCase() === 'keep-alive') || response.hasHeader('keep-alive');
      if (request?.httpVersion === '1.1' && Number.isFinite(keepAliveTimeout) &&
          keepAliveTimeout > 0 && !hasKeepAlive && !closesConnection &&
          (!explicitConnection ||
            entries.some((name, index) => index % 3 === 0 &&
              String(name).toLowerCase() === 'connection' &&
              String(entries[index + 1]).toLowerCase() === 'keep-alive') ||
            String(response.getHeader('connection') || '').toLowerCase() === 'keep-alive')) {
        entries.push('Keep-Alive', `timeout=${Math.floor(keepAliveTimeout / 1000)}`, false);
      }
      const wireEntries = [];
      for (let index = 0; index < entries.length; index += 3) {
        const name = String(entries[index]);
        const value = entries[index + 1];
        const key = name.toLowerCase();
        if (uniqueHeaderNames.has(key) && Array.isArray(value)) {
          wireEntries.push(name, value.join('; '), false);
        } else {
          wireEntries.push(name, value, entries[index + 2]);
        }
      }
      writeHeadOperation(id, status, reason, wireEntries, invalid, response);
      // `writeHead(headers)` stores its explicit headers on the outgoing
      // message alongside values previously added through setHeader(). Keep
      // the public query methods' snapshot in sync after the native operation
      // accepts the write. Map#set preserves existing header order while new
      // names are appended in writeHead order.
      for (let index = 0; index < entries.length; index += 3) {
        const name = String(entries[index]);
        const value = entries[index + 1];
        headers(response).set(name.toLowerCase(), {
          name,
          value: Array.isArray(value) ? value.slice() : value,
        });
      }
      response.statusCode = status;
      response.headersSent = true;
      return response;
    };
    // Early Hints are an informational response and must not commit the final
    // response header lifecycle. Write them directly to the response socket so
    // a subsequent writeHead()/end() still sends the ordinary final status.
    response.writeEarlyHints = (hints) => {
      if (hints === null || typeof hints !== 'object' || Array.isArray(hints)) {
        const error = new TypeError('The "hints" argument must be of type object');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      const lines = [];
      for (const name of Object.keys(hints)) {
        if (!/^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/.test(name)) {
          const error = new TypeError(`Header name must be a valid HTTP token ["${name}"]`);
          error.code = 'ERR_INVALID_HTTP_TOKEN';
          throw error;
        }
        const values = Array.isArray(hints[name]) ? hints[name] : [hints[name]];
        for (const value of values) {
          const text = String(value);
          if (name.toLowerCase() === 'link' &&
              !/^\s*<(?:\/[^<>/\s][^<>\s]*|[^<>/\s][^<>\s]*)>\s*(?:;\s*[^<>\r\n]+)?\s*$/.test(text)) {
            const error = new TypeError(`Invalid value for header "${name}"`);
            error.code = 'ERR_INVALID_ARG_VALUE';
            throw error;
          }
          if (/[\r\n]/.test(text)) {
            const error = new TypeError(`Invalid character in header content ["${name}"]`);
            error.code = 'ERR_INVALID_CHAR';
            throw error;
          }
          lines.push(`${name}: ${text}\r\n`);
        }
      }
      if (lines.length !== 0) response._send(`HTTP/1.1 103 Early Hints\r\n${lines.join('')}\r\n`);
      return this;
    };
    // Install the wrapper as an explicit own property so every constructed
    // response uses this implementation and its status/header validation.
    Object.defineProperty(response, 'writeInformation', {
      configurable: true,
      writable: true,
      value: (statusCode, headerInput, callback) => {
      if (response.headersSent) {
        const error = new Error('Cannot write headers after they are sent to the client');
        error.code = 'ERR_HTTP_HEADERS_SENT';
        throw error;
      }
      if (typeof statusCode !== 'number' || !Number.isFinite(statusCode)) {
        const error = new TypeError('The "statusCode" argument must be of type number');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      if (!Number.isInteger(statusCode) || statusCode < 100 || statusCode > 199) {
        const error = new RangeError('The value of "statusCode" is out of range. It must be an integer between 100 and 199.');
        error.code = 'ERR_OUT_OF_RANGE';
        throw error;
      }
      if (statusCode === 101) {
        const error = new RangeError(`Invalid status code: ${statusCode}`);
        error.code = 'ERR_HTTP_INVALID_STATUS_CODE';
        throw error;
      }

      const reason = statusCode === 100 ? 'Continue' :
        statusCode === 102 ? 'Processing' : statusCode === 103 ? 'Early Hints' : 'unknown';
      let head = `HTTP/1.1 ${statusCode} ${reason}\r\n`;
      if (headerInput !== undefined && headerInput !== null) {
        let entries = [];
        if (Array.isArray(headerInput)) {
          if (headerInput.length > 0 && Array.isArray(headerInput[0])) {
            for (const pair of headerInput) {
              if (!Array.isArray(pair) || pair.length < 2) {
                const error = new TypeError('The "headers" argument must be an object or an array of header pairs');
                error.code = 'ERR_INVALID_ARG_VALUE';
                throw error;
              }
              entries.push(pair[0], pair[1]);
            }
          } else {
            if (headerInput.length % 2 !== 0) {
              const error = new TypeError('The "headers" argument must be an object or an array of header pairs');
              error.code = 'ERR_INVALID_ARG_VALUE';
              throw error;
            }
            entries = headerInput;
          }
        } else if (typeof headerInput === 'object') {
          for (const name of Object.keys(headerInput)) entries.push(name, headerInput[name]);
        } else {
          const error = new TypeError('The "headers" argument must be of type object');
          error.code = 'ERR_INVALID_ARG_TYPE';
          throw error;
        }
        for (let index = 0; index < entries.length; index += 2) {
          const name = String(entries[index]);
          if (!name || /[^!#$%&'*+.^_`|~0-9A-Za-z-]/.test(name)) {
            const error = new TypeError(`Header name must be a valid HTTP token ["${name}"]`);
            error.code = 'ERR_INVALID_HTTP_TOKEN';
            throw error;
          }
          const value = entries[index + 1];
          if (value === undefined) {
            const error = new TypeError(`The "${name}" header content must not be undefined`);
            error.code = 'ERR_HTTP_INVALID_HEADER_VALUE';
            throw error;
          }
          const text = String(value);
          if (invalidInformationHeaderValue.test(text)) {
            const error = new TypeError(`Invalid character in header content ["${name}"]`);
            error.code = 'ERR_INVALID_CHAR';
            throw error;
          }
          head += `${name}: ${text}\r\n`;
        }
      }
      return response._send(`${head}\r\n`, 'ascii', callback);
      },
    });
    response.writeContinue = (callback) => {
      const result = response.writeInformation(100, null, callback);
      response._sent100 = true;
      return result;
    };
    response.writeProcessing = (callback) => response.writeInformation(102, null, callback);
    response.flushHeaders = () => {
      if (!response.headersSent) response.writeHead(response.statusCode);
      return response;
    };
    initializeResponse(id, response);
    return response;
  }
  ServerResponse.prototype = Object.create(Writable.prototype);
  Object.defineProperty(ServerResponse.prototype, "constructor", {
    value: ServerResponse, writable: true, configurable: true,
  });
  // OutgoingMessage.destroy() expects its private socket slot, while these
  // responses own their transport through Writable's host destroy callback.
  ServerResponse.prototype.destroy = function destroy(error) {
    this.destroyed = true;
    if (this.socket) return this.socket.destroy(error);
    return destroyWritable.call(this, error);
  };
  ServerResponse.prototype.assignSocket = function assignSocket(socket) {
    if (this.socket) {
      const error = new Error('Socket is already assigned.');
      error.code = 'ERR_HTTP_SOCKET_ASSIGNED';
      throw error;
    }
    this.socket = socket;
    this.connection = socket;
    return this;
  };
  return ServerResponse;
})"#;

pub(crate) fn module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let status_codes = crate::modules::http_status_shared_vm::codes(context)?;
    let set_header =
        context.host_function(crate::host::shared_vm::operation("httpResponseSetHeader"))?;
    let get_header =
        context.host_function(crate::host::shared_vm::operation("httpResponseGetHeader"))?;
    let remove_header = context.host_function(crate::host::shared_vm::operation(
        "httpResponseRemoveHeader",
    ))?;
    let write = context.host_function_with_data(
        crate::host::shared_vm::operation("httpResponseWrite"),
        status_codes,
    )?;
    let finish = context.host_function_with_data(
        crate::host::shared_vm::operation("httpResponseFinish"),
        status_codes,
    )?;
    let write_head = context.host_function_with_data(
        crate::host::shared_vm::operation("httpResponseWriteHead"),
        status_codes,
    )?;
    let destroy_response =
        context.host_function(crate::host::shared_vm::operation("httpResponseDestroy"))?;
    // Core owns the weak response-to-transport identity. Keeping the method on
    // the prototype preserves ordinary method identity and receiver dispatch.
    let response_data = context.null_object_rooted()?;
    let global = context.global_root()?;
    let key = context.string_rooted("WeakMap");
    let weak_map = context.get_property_rooted(global, key)?;
    let responses = context.construct_rooted(weak_map, weak_map, &[])?;
    let key = context.string_rooted("prototype");
    let prototype = context.get_property_rooted(weak_map, key)?;
    let key = context.string_rooted("set");
    let map_set = context.get_property_rooted(prototype, key)?;
    set(context, response_data, "responses", responses)?;
    set(context, response_data, "set", map_set)?;
    let initialize_response = context.host_function_with_data(
        crate::host::shared_vm::operation("httpResponseInitialize"),
        response_data,
    )?;
    let send = context.host_constructor_with_data(
        crate::host::shared_vm::operation("httpResponseSend"),
        responses,
    )?;
    let key = context.string_rooted("name");
    let name = context.string_rooted("_send");
    context.define_data_property_rooted(send, key, name, false, false, true)?;
    let stream = crate::host::shared_vm::commonjs::stream_module(context)?;
    let writable_key = context.string_rooted("Writable");
    let writable = context.get_property_rooted(stream, writable_key)?;
    let outgoing_message = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "http.js",
        "_http_outgoing.js",
        "OutgoingMessage",
    )?;
    let k_high_water_mark = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "http.js",
        "_http_outgoing.js",
        "kHighWaterMark",
    )?;
    let response_factory =
        context.evaluate_script_rooted(RESPONSE_FACTORY, "node:http/response-factory.js")?;
    let publish_diagnostics =
        context.host_function(crate::host::shared_vm::operation("diagnosticsPublish"))?;
    let undefined = context.undefined();
    let response_factory = context.call_rooted(
        response_factory,
        undefined,
        &[
            writable,
            outgoing_message,
            set_header,
            get_header,
            remove_header,
            write,
            finish,
            write_head,
            destroy_response,
            initialize_response,
            k_high_water_mark,
            publish_diagnostics,
        ],
    )?;
    let key = context.string_rooted("prototype");
    let prototype = context.get_property_rooted(response_factory, key)?;
    set(context, prototype, "_send", send)?;
    let response_factory = context.retain(response_factory)?;
    let global = context.global_root()?;
    let emitter_key = context.string_rooted("__nodeEventEmitter");
    let emitter = context.get_property_rooted(global, emitter_key)?;
    let incoming_constructor = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "http.js",
        "_http_incoming.js",
        "IncomingMessage",
    )?;
    let net = crate::host::shared_vm::commonjs::net_module(context)?;
    let socket_constructor_key = context.string_rooted("Socket");
    let socket_constructor = context.get_property_rooted(net, socket_constructor_key)?;
    context.release_root(socket_constructor_key);
    let incoming_data = context.array_rooted(&[incoming_constructor, socket_constructor])?;
    let incoming_factory = context.host_function_with_data(
        crate::host::shared_vm::operation("httpIncomingCreate"),
        incoming_data,
    )?;
    let incoming_factory = context.retain(incoming_factory)?;

    let initialize_server =
        context.host_function(crate::host::shared_vm::operation("httpCreateServer"))?;
    let request = super::client::request_function(context)?;
    let create_agent =
        context.host_function(crate::host::shared_vm::operation("httpAgentCreate"))?;
    let destroy_agent =
        context.host_function(crate::host::shared_vm::operation("httpAgentDestroy"))?;
    let connect = get(context, net, "connect")?;
    // Node's helper constructs ConnectionsList from internalBinding('http_parser')
    // and starts a JS timeout scan for Node's own HTTPParser. Quench's shared
    // HTTP poll owns accepted sockets and timeout handling, while close(),
    // closeAllConnections(), and closeIdleConnections() below are host-backed.
    // Keep the listening hook expected by Server, but don't call the Node
    // helper: its internal ConnectionsList constructor is not part of Quench's
    // public/shared-VM HTTP parser surface.
    let connections_checking_interval = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "http.js",
        "_http_server.js",
        "kConnectionsCheckingInterval",
    )?;
    let connection_listener = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "http.js",
        "_http_server.js",
        "_connectionListener",
    )?;
    // The actual Node helper scans its own HTTPParser connection list. Quench
    // owns connection/request timeout scanning in the shared HTTP poll, so we
    // only need the interval's public lifecycle here. Node exposes this timer
    // through kConnectionsCheckingInterval, and Server.close() destroys it.
    let setup_connections_tracking_factory = context.evaluate_script_rooted(
        r#"(key) => function setupConnectionsTracking() {
          const previous = this[key];
          if (previous) clearInterval(previous);
          const interval = setInterval(() => {}, this.connectionsCheckingInterval);
          interval.unref();
          this[key] = interval;
        }"#,
        "node:http/setup-connections-tracking.js",
    )?;
    let undefined = context.undefined();
    let setup_connections_tracking = context.call_rooted(
        setup_connections_tracking_factory,
        undefined,
        &[connections_checking_interval],
    )?;
    context.release_root(setup_connections_tracking_factory);
    let factory = context.evaluate_script_rooted(MODULE_FACTORY, "node:http/shared-api.js")?;
    let module = context.call_rooted(
        factory,
        undefined,
        &[
            initialize_server,
            emitter,
            request,
            create_agent,
            destroy_agent,
            connect,
            incoming_constructor,
            response_factory,
            k_high_water_mark,
            setup_connections_tracking,
            connections_checking_interval,
            connection_listener,
        ],
    )?;
    let request_key = context.string_rooted("request");
    let request_constructor = context.get_property_rooted(module, request_key)?;
    set(context, module, "ClientRequest", request_constructor)?;
    let implicit_header = context.evaluate_script_rooted(
        "(function _implicitHeader() {})",
        "node:http/implicit-header.js",
    )?;
    for constructor in [request_constructor, response_factory] {
        let key = context.string_rooted("prototype");
        let prototype = context.get_property_rooted(constructor, key)?;
        set(context, prototype, "_implicitHeader", implicit_header)?;
    }
    let decorate_outgoing = context.evaluate_script_rooted(
        r#"(Constructor) => {
          const headersByMessage = new WeakMap();
          const erroredGetter = Object.getOwnPropertyDescriptor(Constructor.prototype, 'errored')?.get;
          const headers = (message) => {
            let value = headersByMessage.get(message);
            if (value === undefined) {
              value = Object.create(null);
              headersByMessage.set(message, value);
            }
            return value;
          };
          const invalidHeaderName = (name) => {
            const error = new TypeError(`Header name must be a valid HTTP token ["${name}"]`);
            error.code = 'ERR_INVALID_HTTP_TOKEN';
            return error;
          };
          const invalidHeaderValue = (name, value) => {
            const error = new TypeError(`Invalid character in header content ["${name}"]`);
            error.code = 'ERR_INVALID_CHAR';
            return error;
          };
          const validateName = (name) => {
            if (typeof name !== 'string' || !/^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/.test(name)) {
              throw invalidHeaderName(name);
            }
          };
          Object.defineProperties(Constructor.prototype, {
            setHeader: { configurable: true, writable: true, value: function setHeader(name, value) {
              if (this._header) {
                const error = new Error('Cannot set headers after they are sent to the client');
                error.code = 'ERR_HTTP_HEADERS_SENT';
                throw error;
              }
              validateName(name);
              if (value === undefined) {
                const error = new TypeError(`Invalid value \"${value}\" for header \"${name}\"`);
                error.code = 'ERR_HTTP_INVALID_HEADER_VALUE';
                throw error;
              }
              const values = Array.isArray(value) ? value.map(String) : String(value);
              const check = Array.isArray(values) ? values.join('') : values;
              if (/[^\t\x20-\x7e]/.test(check)) throw invalidHeaderValue(name, value);
              headers(this)[name.toLowerCase()] = [name, values];
              return this;
            } },
            getHeader: { configurable: true, writable: true, value: function getHeader(name) {
              return headers(this)[String(name).toLowerCase()]?.[1];
            } },
            getHeaders: { configurable: true, writable: true, value: function getHeaders() {
              const result = Object.create(null);
              const values = headers(this);
              for (const name of Object.keys(values)) result[name] = values[name][1];
              return result;
            } },
            getHeaderNames: { configurable: true, writable: true, value: function getHeaderNames() {
              return Object.keys(headers(this));
            } },
            getRawHeaderNames: { configurable: true, writable: true, value: function getRawHeaderNames() {
              return Object.keys(headers(this)).map((name) => headers(this)[name][0]);
            } },
            hasHeader: { configurable: true, writable: true, value: function hasHeader(name) {
              return Object.hasOwn(headers(this), String(name).toLowerCase());
            } },
            removeHeader: { configurable: true, writable: true, value: function removeHeader(name) {
              validateName(name);
              delete headers(this)[name.toLowerCase()];
            } },
            appendHeader: { configurable: true, writable: true, value: function appendHeader(name, value) {
              validateName(name);
              const values = headers(this);
              const key = name.toLowerCase();
              const previous = values[key];
              const next = Array.isArray(value) ? value : [value];
              if (previous === undefined) values[key] = [name, next];
              else previous[1] = (Array.isArray(previous[1]) ? previous[1] : [previous[1]]).concat(next);
              return this;
            } },
            addTrailers: { configurable: true, writable: true, value: function addTrailers(trailers) {
              if (trailers === null || typeof trailers !== 'object') {
                throw new TypeError('The "headers" argument must be of type object');
              }
              for (const name of Object.keys(trailers)) {
                if (typeof name !== 'string' || !/^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/.test(name)) {
                  const error = new TypeError(`Trailer name must be a valid HTTP token ["${name}"]`);
                  error.code = 'ERR_INVALID_HTTP_TOKEN';
                  throw error;
                }
                const value = Array.isArray(trailers[name]) ? trailers[name].join('') : String(trailers[name]);
                if (/[^\t\x20-\x7e]/.test(value)) {
                  const error = new TypeError(`Invalid character in trailer content ["${name}"]`);
                  error.code = 'ERR_INVALID_CHAR';
                  throw error;
                }
              }
              this._trailer = { ...trailers };
              const uniqueHeaders = new Set(Array.isArray(this.uniqueHeaders)
                ? this.uniqueHeaders.map((name) => String(name).toLowerCase())
                : []);
              const entries = [];
              for (const name of Object.keys(trailers)) {
                const values = Array.isArray(trailers[name])
                  ? trailers[name].map(String)
                  : [String(trailers[name])];
                if (uniqueHeaders.has(name.toLowerCase())) {
                  entries.push(name, values.join('; '));
                } else {
                  for (const value of values) entries.push(name, value);
                }
              }
              this.__quenchTrailerEntries = entries;
            } },
          });
          function DecoratedOutgoingMessage(...args) {
            const message = Reflect.construct(Constructor, args, new.target || DecoratedOutgoingMessage);
            if (erroredGetter) {
              Object.defineProperty(message, 'errored', {
                configurable: true,
                get() {
                  const error = erroredGetter.call(message);
                  return error === null ? undefined : error;
                },
              });
            }
            return message;
          }
          Object.setPrototypeOf(DecoratedOutgoingMessage, Constructor);
          DecoratedOutgoingMessage.prototype = Constructor.prototype;
          return DecoratedOutgoingMessage;
        }"#,
        "node:http/outgoing-message-header-api.js",
    )?;
    let undefined = context.undefined();
    let outgoing_message =
        context.call_rooted(decorate_outgoing, undefined, &[outgoing_message])?;
    set(context, module, "OutgoingMessage", outgoing_message)?;
    // ClientRequest is an OutgoingMessage in Node. The shared HTTP request
    // implementation is backed by Writable, so retain the Writable methods
    // on its private prototype while linking it into Node's public prototype
    // chain for `instanceof http.OutgoingMessage` and diagnostics payloads.
    let request_prototype = get(context, request_constructor, "prototype")?;
    let writable_prototype = get(context, writable, "prototype")?;
    let outgoing_prototype = get(context, outgoing_message, "prototype")?;
    let link_client_request = context.evaluate_script_rooted(
        r#"(requestPrototype, writablePrototype, outgoingPrototype) => {
          for (const key of Reflect.ownKeys(writablePrototype)) {
            if (key === 'constructor' || Object.hasOwn(requestPrototype, key)) continue;
            Object.defineProperty(
              requestPrototype,
              key,
              Object.getOwnPropertyDescriptor(writablePrototype, key),
            );
          }
          Object.setPrototypeOf(requestPrototype, outgoingPrototype);
        }"#,
        "node:http/client-request-prototype.js",
    )?;
    context.call_rooted(
        link_client_request,
        undefined,
        &[request_prototype, writable_prototype, outgoing_prototype],
    )?;
    let response = get(context, module, "ServerResponse")?;
    let response_prototype = get(context, response, "prototype")?;
    let outgoing_prototype = get(context, outgoing_message, "prototype")?;
    context.set_prototype_rooted(response_prototype, outgoing_prototype)?;
    set(context, module, "STATUS_CODES", status_codes)?;
    let net_server = get(context, net, "Server")?;
    let server = get(context, module, "Server")?;
    let net_prototype = get(context, net_server, "prototype")?;
    let server_prototype = get(context, server, "prototype")?;
    context.set_prototype_rooted(server, net_server)?;
    context.set_prototype_rooted(server_prototype, net_prototype)?;
    let key = context.string_rooted("Agent");
    let agent = context.get_property_rooted(module, key)?;
    let key = context.string_rooted("prototype");
    let prototype = context.get_property_rooted(agent, key)?;
    let native = context.host_function(crate::host::shared_vm::operation("getName"))?;
    let wrapper = context.evaluate_script_rooted(
        "native => function getName(options = undefined) { return native(options); }",
        "node:http/agent-get-name.js",
    )?;
    let get_name = context.call_rooted(wrapper, undefined, &[native])?;
    set(context, prototype, "getName", get_name)?;
    for (name, public_operation, original_operation) in [
        (
            "validateHeaderName",
            "httpValidateHeaderNamePublic",
            "httpValidateHeaderNameOriginal",
        ),
        (
            "validateHeaderValue",
            "httpValidateHeaderValuePublic",
            "httpValidateHeaderValueOriginal",
        ),
    ] {
        let public =
            context.host_constructor(crate::host::shared_vm::operation(public_operation))?;
        let original =
            context.host_function(crate::host::shared_vm::operation(original_operation))?;
        for (function, name) in [(public, name), (original, "")] {
            let key = context.string_rooted("name");
            let name = context.string_rooted(name);
            context.define_data_property_rooted(function, key, name, false, false, true)?;
        }
        set(context, public, "withoutStackTrace", original)?;
        set(context, module, name, public)?;
    }
    let methods = crate::modules::http_protocol::HTTP_METHODS
        .iter()
        .map(|method| context.string_rooted(method))
        .collect::<Vec<_>>();
    let methods = context.array_rooted(&methods)?;
    set(context, module, "METHODS", methods)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
        .response_factory = Some(response_factory);
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
        .incoming_factory = Some(incoming_factory);
    // Match Node's lazy getters in lib/http.js. Undici imports `node:http`
    // during initialization, so loading it while this builtin is being
    // constructed would create a partial module cycle. Defer the load until
    // one of these properties is actually read.
    let initialize_fetch = context.host_function(crate::host::shared_vm::operation(
        "webFetchInitialize",
    ))?;
    let global = context.global_root()?;
    let install_websocket_getters = context.evaluate_script_rooted(
        r#"(module, initializeFetch, global) => {
          for (const name of ['WebSocket', 'CloseEvent', 'MessageEvent']) {
            Object.defineProperty(module, name, {
              configurable: true,
              enumerable: true,
              get() {
                initializeFetch();
                return global[name];
              },
            });
          }
        }"#,
        "node:http/undici-lazy-getters.js",
    )?;
    let undefined = context.undefined();
    context.call_rooted(
        install_websocket_getters,
        undefined,
        &[module, initialize_fetch, global],
    )?;
    Ok(module)
}

pub(crate) fn create_server(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let packed = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("shared HTTP server arguments are missing"))?;
    let server = array_item(context, packed, 0)?;
    let listener = array_item(context, packed, 1)?;
    if context.is_callable_rooted(listener)? {
        let on_key = context.string_rooted("on");
        let on = context.get_property_rooted(server, on_key)?;
        let event = context.string_rooted("request");
        context.call_rooted(on, server, &[event, listener])?;
    }
    let id = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
        .server_id()
        .map_err(RootedError::host)?;
    let id_value = context.number(id as f64);
    for (name, operation) in [
        ("listen", "httpServerListen"),
        ("address", "httpServerAddress"),
        ("close", "httpServerClose"),
        ("closeAllConnections", "httpServerClose"),
        ("closeIdleConnections", "httpServerClose"),
    ] {
        let function = context
            .host_function_with_data(crate::host::shared_vm::operation(operation), id_value)?;
        let function = if name == "closeAllConnections" || name == "closeIdleConnections" {
            let mode = context.string_rooted(if name == "closeAllConnections" {
                "all"
            } else {
                "idle"
            });
            let factory = context.evaluate_script_rooted(
                "((close, mode) => function() { close(mode); })",
                "node:http/server-close-connections.js",
            )?;
            let undefined = context.undefined();
            let wrapped = context.call_rooted(factory, undefined, &[function, mode])?;
            context.release_root(factory);
            wrapped
        } else if name == "close" {
            let factory = context
                .evaluate_script_rooted(SERVER_CLOSE_FACTORY, "node:http/server-close.js")?;
            let undefined = context.undefined();
            let wrapped = context.call_rooted(factory, undefined, &[function])?;
            context.release_root(factory);
            wrapped
        } else {
            function
        };
        set(context, server, name, function)?;
    }
    let unref = context.boolean(false);
    set(context, server, "_unref", unref)?;
    let handle = context.null();
    set(context, server, "_handle", handle)?;
    let retained = context.retain(server)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
        .servers
        .insert(
            id,
            super::state::Server {
                root: retained,
                listener: None,
                listening_pending: false,
                closing: false,
                connections: Default::default(),
            },
        );
    Ok(server)
}

pub(crate) fn server_listen(
    context: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = function_data_id(context)?;
    let first = args.first().copied();
    let first_is_callback = match first {
        Some(root) => context.is_callable_rooted(root)?,
        None => false,
    };
    let address = listen_address(context, first, first_is_callback, args.get(1).copied())?;
    let options =
        first.filter(|root| context.is_object_rooted(*root).unwrap_or(false) && !first_is_callback);
    let backlog = if options.is_some() {
        net_shared_vm::listen_backlog(context, options)?
    } else if matches!(&address, HttpListenAddress::Local(_))
        && args.get(1).copied().is_some_and(|value| {
            !context.is_callable_rooted(value).unwrap_or(false)
                && context.truthy_rooted(value).unwrap_or(false)
        })
    {
        net_shared_vm::convert_listen_backlog(context, args[1])?
    } else {
        net_shared_vm::listen_backlog(context, None)?
    };
    let host = context.host_mut().shared_state();
    {
        let guard = host.borrow();
        let Some(server) = guard.http.servers.get(&id) else {
            return Err(RootedError::host("shared HTTP server is closed"));
        };
        if server.listener.is_some() {
            return Err(RootedError::host("shared HTTP server is already listening"));
        }
    }
    let listener = match address {
        HttpListenAddress::Tcp(address) => {
            let result =
                net_shared_vm::listen_with_backlog(&mut host.borrow_mut().tcp, address, backlog);
            match result {
                Ok(listener) => listener,
                Err(error) => {
                    return queue_http_listen_error(context, receiver, error, address);
                }
            }
        }
        #[cfg(unix)]
        HttpListenAddress::Local(path) => {
            net_shared_vm::listen_local_endpoint(context, &path, backlog)
                .map_err(RootedError::host)?
        }
    };
    {
        let mut guard = host.borrow_mut();
        let server = guard.http.servers.get_mut(&id).unwrap();
        server.listener = Some(listener);
        server.listening_pending = true;
    }
    let listener = host.borrow().http.servers[&id].listener.unwrap();
    crate::modules::net_reference_shared_vm::install(context, listener, receiver)?;
    let callback = if first_is_callback {
        first
    } else if let Some(second) = args.get(1).copied() {
        if context.is_callable_rooted(second)? {
            Some(second)
        } else {
            args.get(2).copied()
        }
    } else {
        args.get(2).copied()
    };
    if let Some(callback) = callback {
        if context.is_callable_rooted(callback)? {
            let on_key = context.string_rooted("on");
            let on = context.get_property_rooted(receiver, on_key)?;
            let event = context.string_rooted("listening");
            context.call_rooted(on, receiver, &[event, callback])?;
        }
    }
    Ok(receiver)
}

fn queue_http_listen_error(
    context: &mut Context<'_>,
    server: RootId,
    error: io::Error,
    address: SocketAddr,
) -> Result<RootId, RootedError> {
    let errno = error.raw_os_error().map(|value| -f64::from(value));
    let code = errno
        .map(crate::modules::util_system_error_shared_vm::system_error_name)
        .unwrap_or_else(|| "UNKNOWN".to_owned());
    let reason = error.to_string();
    let reason = reason
        .split(" (os error")
        .next()
        .unwrap_or(&reason)
        .to_lowercase();
    let error = context.error_rooted(&format!(
        "listen {code}: {reason} {}:{}",
        address.ip(),
        address.port()
    ))?;
    for (name, value) in [("code", code.as_str()), ("syscall", "listen")] {
        let key = context.string_rooted(name);
        let value = context.string_rooted(value);
        context.define_data_property_rooted(error, key, value, true, true, true)?;
    }
    let key = context.string_rooted("address");
    let value = context.string_rooted(&address.ip().to_string());
    context.define_data_property_rooted(error, key, value, true, true, true)?;
    let key = context.string_rooted("port");
    let value = context.number(f64::from(address.port()));
    context.define_data_property_rooted(error, key, value, true, true, true)?;
    if let Some(errno) = errno {
        let key = context.string_rooted("errno");
        let value = context.number(errno);
        context.define_data_property_rooted(error, key, value, true, true, true)?;
    }
    let callback = context.evaluate_script_rooted(
        "(server, error) => server.emit('error', error)",
        "node:http/server-listen-error.js",
    )?;
    let undefined = context.undefined();
    crate::modules::shared_event_loop::queue_next_tick(
        context,
        callback,
        undefined,
        &[server, error],
    )?;
    context.release_root(callback);
    Ok(server)
}

fn listen_address(
    context: &mut Context<'_>,
    first: Option<RootId>,
    first_is_callback: bool,
    second: Option<RootId>,
) -> Result<HttpListenAddress, RootedError> {
    let Some(root) = first.filter(|_| !first_is_callback) else {
        return resolve_listen_address(DEFAULT_LISTEN_HOST, 0).map(HttpListenAddress::Tcp);
    };
    let value = context
        .rooted_value(root)
        .ok_or_else(|| RootedError::host("shared HTTP listen argument is unavailable"))?;
    if let Some(port) = value.as_number() {
        return resolve_listen_address(&listen_host(context, second)?, numeric_port(port)?)
            .map(HttpListenAddress::Tcp);
    }
    if let Some(path) = context.string_text(root)? {
        #[cfg(unix)]
        return Ok(HttpListenAddress::Local(path));
        #[cfg(not(unix))]
        return Err(invalid_listen_port());
    }
    if value.is_null() || value.as_bool().is_some() {
        return Err(invalid_listen_port());
    }
    let path_key = context.string_rooted("path");
    let path = context.get_property_rooted(root, path_key)?;
    if let Some(path) = context.string_text(path)? {
        #[cfg(unix)]
        return Ok(HttpListenAddress::Local(path));
        #[cfg(not(unix))]
        return Err(invalid_listen_port());
    }
    let key = context.string_rooted("port");
    let port = context.get_property_rooted(root, key)?;
    let port = listen_port(context, port)?;
    let host_key = context.string_rooted("host");
    let host = context.get_property_rooted(root, host_key)?;
    let host = match context.rooted_value(host) {
        Some(value) if value.is_undefined() || value.is_null() => DEFAULT_LISTEN_HOST.to_owned(),
        _ => context.string_text(host)?.ok_or_else(invalid_listen_port)?,
    };
    resolve_listen_address(&host, port).map(HttpListenAddress::Tcp)
}

fn listen_host(context: &mut Context<'_>, value: Option<RootId>) -> Result<String, RootedError> {
    let Some(value) = value else {
        return Ok(DEFAULT_LISTEN_HOST.to_owned());
    };
    if context.is_callable_rooted(value)? {
        return Ok(DEFAULT_LISTEN_HOST.to_owned());
    }
    match context.rooted_value(value) {
        Some(value) if value.is_undefined() || value.is_null() => {
            Ok(DEFAULT_LISTEN_HOST.to_owned())
        }
        Some(value) if value.as_bool().is_some() || value.as_number().is_some() => {
            Err(RootedError::host("invalid HTTP listen host"))
        }
        _ => context
            .string_text(value)?
            .ok_or_else(|| RootedError::host("invalid HTTP listen host")),
    }
}

fn listen_port(context: &mut Context<'_>, root: RootId) -> Result<u16, RootedError> {
    let value = context
        .rooted_value(root)
        .ok_or_else(|| RootedError::host("shared HTTP listen port is unavailable"))?;
    if value.is_undefined() {
        return Ok(0);
    }
    if let Some(port) = value.as_number() {
        return numeric_port(port);
    }
    if let Some(port) = context.string_text(root)? {
        return string_port(&port);
    }
    Err(invalid_listen_port())
}

fn resolve_listen_address(host: &str, port: u16) -> Result<SocketAddr, RootedError> {
    (host, port)
        .to_socket_addrs()
        .map_err(|error| RootedError::host(format!("cannot resolve HTTP listen host: {error}")))?
        .next()
        .ok_or_else(|| RootedError::host("HTTP listen host resolved to no addresses"))
}

fn numeric_port(port: f64) -> Result<u16, RootedError> {
    if port.is_finite() && port.fract() == 0.0 && (0.0..=u16::MAX as f64).contains(&port) {
        Ok(port as u16)
    } else {
        Err(invalid_listen_port())
    }
}

fn string_port(port: &str) -> Result<u16, RootedError> {
    port.parse().map_err(|_| invalid_listen_port())
}

fn invalid_listen_port() -> RootedError {
    RootedError::host("shared HTTP listen requires a numeric port or { port, host } options")
}

pub(crate) fn server_address(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = function_data_id(context)?;
    let host = context.host_mut().shared_state();
    let (local_path, address) = {
        let guard = host.borrow();
        let listener = guard
            .http
            .servers
            .get(&id)
            .and_then(|server| server.listener);
        (
            listener.and_then(|listener| {
                net_shared_vm::listener_path(&guard.tcp, listener).map(str::to_owned)
            }),
            listener.and_then(|listener| net_shared_vm::address(&guard.tcp, listener)),
        )
    };
    if let Some(path) = local_path {
        return Ok(context.string_rooted(&path));
    }
    let Some(address) = address else {
        return Ok(context.null());
    };
    let object = context.object_rooted()?;
    let address_value = context.string_rooted(&address.ip().to_string());
    set(context, object, "address", address_value)?;
    let family = context.string_rooted(match address.ip() {
        IpAddr::V4(_) => "IPv4",
        IpAddr::V6(_) => "IPv6",
    });
    set(context, object, "family", family)?;
    let port = context.number(address.port() as f64);
    set(context, object, "port", port)?;
    Ok(object)
}

pub(crate) fn server_close(
    context: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = function_data_id(context)?;
    let connection_mode = match args
        .first()
        .and_then(|root| context.string_text(*root).ok().flatten())
    {
        Some(mode) if mode == "all" || mode == "idle" => Some(mode),
        _ => None,
    };
    let host = context.host_mut().shared_state();
    if let Some(mode) = connection_mode {
        let mut guard = host.borrow_mut();
        let sockets = guard
            .http
            .servers
            .get(&id)
            .map(|server| {
                server
                    .connections
                    .iter()
                    .copied()
                    .filter(|socket| {
                        mode == "all"
                            || guard
                                .http
                                .connections
                                .get(socket)
                                .is_some_and(|connection| {
                                    connection.received.is_empty()
                                        && (!connection.request_dispatched
                                            || guard
                                                .http
                                                .responses
                                                .values()
                                                .filter(|response| response.socket == *socket)
                                                .all(|response| response.lifecycle.is_terminal()))
                                })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for socket in sockets {
            net_shared_vm::close_socket(&mut guard.tcp, socket);
        }
        return Ok(context.undefined());
    }
    {
        let mut guard = host.borrow_mut();
        let mut idle_sockets = Vec::new();
        let listener = guard.http.servers.get_mut(&id).and_then(|server| {
            server.closing = true;
            server.listening_pending = false;
            idle_sockets.extend(server.connections.iter().copied());
            server.listener.take()
        });
        if let Some(listener) = listener {
            net_shared_vm::close_listener(&mut guard.tcp, listener);
        }
        idle_sockets.retain(|socket| {
            guard
                .http
                .connections
                .get(socket)
                .is_some_and(|connection| {
                    connection.received.is_empty()
                        && (!connection.request_dispatched
                            || guard
                                .http
                                .responses
                                .values()
                                .filter(|response| response.socket == *socket)
                                .all(|response| response.lifecycle.is_terminal()))
                })
        });
        for socket in idle_sockets {
            // A response can be terminal while its bytes are still queued in
            // the shared transport. Drain those bytes before half-closing;
            // otherwise an already-idle connection can be closed at once.
            if net_shared_vm::socket_write_queue_size(&guard.tcp, socket) == 0 {
                net_shared_vm::close_socket(&mut guard.tcp, socket);
            } else {
                net_shared_vm::end(&mut guard.tcp, socket, false).map_err(RootedError::host)?;
            }
        }
    }
    let handle = context.null();
    set(context, receiver, "_handle", handle)?;
    Ok(receiver)
}

pub(crate) fn agent_create(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let options = args.first().copied().unwrap_or_else(|| context.undefined());
    let max_sockets = {
        let key = context.string_rooted("maxSockets");
        let value = context.get_property_rooted(options, key)?;
        context
            .rooted_value(value)
            .and_then(Value::as_number)
            .filter(|value| value.is_finite() && *value > 0.0)
            .map(|value| value as usize)
            .unwrap_or(15)
    };
    let keep_alive = {
        let key = context.string_rooted("keepAlive");
        let value = context.get_property_rooted(options, key)?;
        context.truthy_rooted(value)?
    };
    let state = context.host_mut().shared_state();
    let mut http = state.borrow_mut();
    let id = http.http.agent_id().map_err(RootedError::host)?;
    http.http.agent_max_sockets.insert(id, max_sockets);
    http.http.agent_keep_alive.insert(id, keep_alive);
    Ok(context.number(id as f64))
}

/// Node's pooling key, preserving property read order and guest coercions.
pub(crate) fn agent_get_name(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let options = match args.first().copied() {
        Some(value) if !context.rooted_value(value).is_some_and(Value::is_undefined) => value,
        _ => context.null_object_rooted()?,
    };
    let get = |context: &mut Context<'_>, name: &str| {
        let key = context.string_rooted(name);
        context.get_property_rooted(options, key)
    };
    let host = get(context, "host")?;
    let mut name = if context.truthy_rooted(host)? {
        host
    } else {
        context.string_rooted("localhost")
    };
    for property in ["port", "localAddress"] {
        let colon = context.string_rooted(":");
        name = context.arithmetic_rooted(NativeArithmetic::Add, name, colon)?;
        let value = get(context, property)?;
        if context.truthy_rooted(value)? {
            let value = get(context, property)?;
            name = context.arithmetic_rooted(NativeArithmetic::Add, name, value)?;
        }
    }
    let family = get(context, "family")?;
    let ipv4 = context.rooted_value(family).and_then(Value::as_number) == Some(4.0);
    let ipv6 = if ipv4 {
        false
    } else {
        let family = get(context, "family")?;
        context.rooted_value(family).and_then(Value::as_number) == Some(6.0)
    };
    if ipv4 || ipv6 {
        let family = get(context, "family")?;
        let text = context.to_string(family)?;
        let suffix = context.string_rooted(&format!(":{text}"));
        name = context.arithmetic_rooted(NativeArithmetic::Add, name, suffix)?;
    }
    let socket = get(context, "socketPath")?;
    if context.truthy_rooted(socket)? {
        let socket = get(context, "socketPath")?;
        let text = context.to_string(socket)?;
        let suffix = context.string_rooted(&format!(":{text}"));
        name = context.arithmetic_rooted(NativeArithmetic::Add, name, suffix)?;
    }
    Ok(name)
}

pub(crate) fn agent_destroy(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(agent) = args
        .first()
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && *id >= 0.0)
        .map(|id| id as u64)
    else {
        return Ok(context.undefined());
    };
    let (aborted_clients, pending) = {
        let host = context.host_mut().shared_state();
        let mut guard = host.borrow_mut();
        let Some(sockets) = guard.http.agents.remove(&agent) else {
            return Ok(context.undefined());
        };
        guard.http.agent_max_sockets.remove(&agent);
        guard.http.agent_keep_alive.remove(&agent);
        guard
            .http
            .agent_endpoints
            .retain(|(owner, _), _| *owner != agent);
        let pending = guard.http.agent_pending.remove(&agent).unwrap_or_default();
        let mut aborted_clients = Vec::new();
        for socket in sockets {
            let response_is_complete = guard
                .http
                .clients
                .get(&socket)
                .is_some_and(|client| client.response_parser.is_complete());
            if response_is_complete {
                // The poller has parsed the full response and may currently be
                // emitting its head or body. Let that terminal transition finish;
                // it will close the now-unowned socket after guest callbacks return.
                continue;
            }
            net_shared_vm::close_socket(&mut guard.tcp, socket);
            if let Some(client) = guard.http.clients.remove(&socket) {
                aborted_clients.push(client);
            }
            guard
                .http
                .responses
                .retain(|_, response| response.socket != socket);
        }
        (aborted_clients, pending)
    };
    for pending in pending {
        context.release_root(pending.request_root);
    }
    let mut first_error = None;
    for client in aborted_clients {
        if let Err(error) = super::client::abort_exchange(context, client) {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(context.undefined())
}

pub(crate) fn response_set_header(
    context: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let name_root = argument(context, args.get(1).copied());
    let value_root = argument(context, args.get(2).copied());
    let name = header_name(context, name_root)?;
    let multiple = args
        .get(3)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let relaxed = args
        .get(4)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let values = header_values(context, value_root, multiple, &name, relaxed)?;
    let lifecycle = context
        .host_mut()
        .shared_state()
        .borrow()
        .http
        .responses
        .get(&id)
        .map(|response| response.lifecycle);
    if lifecycle.is_some_and(|state| state != super::state::ResponseLifecycle::Open) {
        return throw_response_error(
            context,
            false,
            "ERR_HTTP_HEADERS_SENT",
            "Cannot set headers after they are sent to the client",
        );
    }
    let state = context.host_mut().shared_state();
    let mut host = state.borrow_mut();
    let response = host
        .http
        .responses
        .get_mut(&id)
        .ok_or_else(|| RootedError::host("shared HTTP response is no longer active"))?;
    response.removed_headers.remove(&name.to_ascii_lowercase());
    response
        .headers
        .retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
    let values = limit_header_values(&name, values);
    response
        .headers
        .extend(values.into_iter().map(|value| (name.clone(), value)));
    Ok(receiver)
}

pub(crate) fn response_remove_header(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let name_root = argument(context, args.get(1).copied());
    let name = context.to_string(name_root)?;
    let state = context.host_mut().shared_state();
    let mut host = state.borrow_mut();
    if let Some(response) = host.http.responses.get_mut(&id) {
        if response.lifecycle != super::state::ResponseLifecycle::Open {
            drop(host);
            return throw_response_error(
                context,
                false,
                "ERR_HTTP_HEADERS_SENT",
                "Cannot remove headers after they are sent to the client",
            );
        }
        response
            .headers
            .retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
        response.removed_headers.insert(name.to_ascii_lowercase());
        if name.eq_ignore_ascii_case("date") {
            response.send_date = false;
        }
    }
    Ok(context.undefined())
}

pub(crate) fn response_get_header(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let name_root = argument(context, args.get(1).copied());
    let name = context.to_string(name_root)?;
    let values = context
        .host_mut()
        .shared_state()
        .borrow()
        .http
        .responses
        .get(&id)
        .map(|response| {
            response
                .headers
                .iter()
                .filter(|(header, _)| header.eq_ignore_ascii_case(&name))
                .map(|(_, value)| value.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    match values.len() {
        0 => Ok(context.undefined()),
        1 => Ok(context.string_rooted(&values[0])),
        _ => {
            let values = values
                .iter()
                .map(|value| context.string_rooted(value))
                .collect::<Vec<_>>();
            context.array_rooted(&values)
        }
    }
}

pub(crate) fn response_initialize(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument(context, args.first().copied());
    let response = argument(context, args.get(1).copied());
    let data = context.host_function_data()?;
    let key = context.string_rooted("responses");
    let responses = context.get_property_rooted(data, key)?;
    let key = context.string_rooted("set");
    let map_set = context.get_property_rooted(data, key)?;
    context.call_rooted(map_set, responses, &[response, id])?;
    let yes = context.boolean(true);
    set(context, response, "sendDate", yes)?;
    let undefined = context.undefined();
    set(context, response, "statusMessage", undefined)?;
    Ok(context.undefined())
}

fn response_chunk_bytes(
    context: &mut Context<'_>,
    chunk: RootId,
    encoding: Option<RootId>,
) -> Result<Vec<u8>, RootedError> {
    let bytes = match context.view_bytes_rooted(chunk) {
        Some(bytes) => bytes,
        None => {
            let encoding = match encoding {
                Some(value) if context.truthy_rooted(value)? => context.to_string(value)?,
                _ => "utf8".to_owned(),
            };
            let Some(encoding_name) = crate::modules::buffer_codec::canonical_encoding(&encoding)
            else {
                return throw_response_error(
                    context,
                    true,
                    "ERR_UNKNOWN_ENCODING",
                    &format!("Unknown encoding: {encoding}"),
                );
            };
            let units = context.to_string_code_units(chunk)?;
            crate::modules::buffer_codec::encode_units(&units, encoding_name)
        }
    };
    Ok(bytes)
}

pub(crate) fn response_send(
    context: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let responses = context.host_function_data()?;
    let id = context.weak_map_get_rooted(responses, receiver)?;
    let id = argument_id(context, Some(id))?;
    let chunk = argument(context, args.first().copied());
    let bytes = response_chunk_bytes(context, chunk, args.get(1).copied())?;
    let shared = context.host_mut().shared_state();
    let socket = shared
        .borrow()
        .http
        .responses
        .get(&id)
        .map(|response| response.socket);
    let Some(socket) = socket else {
        return Ok(context.boolean(false));
    };
    if !bytes.is_empty() {
        net_shared_vm::write(&mut shared.borrow_mut().tcp, socket, &bytes)
            .map_err(RootedError::host)?;
    }
    if let Some(callback) = args.get(2).copied() {
        if context.is_callable_rooted(callback)? {
            crate::modules::process_shared_vm::next_tick(context, receiver, &[callback])?;
        }
    }
    Ok(context.boolean(true))
}

// Public sendDate is consulted when headers are serialized, not after a response
// has already committed them. Header removal can independently suppress it.
fn response_send_date(
    context: &mut Context<'_>,
    id: u64,
    response: Option<RootId>,
) -> Result<(), RootedError> {
    let shared = context.host_mut().shared_state();
    let open = shared
        .borrow()
        .http
        .responses
        .get(&id)
        .is_some_and(|response| response.lifecycle == super::state::ResponseLifecycle::Open);
    if open {
        if let Some(response) = response {
            let key = context.string_rooted("sendDate");
            let value = context.get_property_rooted(response, key)?;
            let enabled = context.truthy_rooted(value)?;
            if !enabled {
                if let Some(response) = shared.borrow_mut().http.responses.get_mut(&id) {
                    response.send_date = false;
                }
            }
        }
    }
    Ok(())
}

fn response_status(
    context: &mut Context<'_>,
    id: u64,
    response: Option<RootId>,
    status: Option<RootId>,
    reason: Option<RootId>,
) -> Result<(u16, Vec<u8>), RootedError> {
    let open = context
        .host_mut()
        .shared_state()
        .borrow()
        .http
        .responses
        .get(&id)
        .is_some_and(|response| response.lifecycle == super::state::ResponseLifecycle::Open);
    if !open {
        return Ok((200, Vec::new()));
    }
    let response = argument(context, response);
    let status = argument(context, status);
    let reason = argument(context, reason);
    let status = crate::modules::http_status_shared_vm::prepare(
        context,
        response,
        status,
        crate::modules::http_status_shared_vm::Reason::Existing(reason),
    )?;
    let reason = crate::modules::http_status_shared_vm::encode(context, response, status)?;
    Ok((status, reason))
}

pub(crate) fn response_write(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    if id == 0 {
        return Ok(context.boolean(true));
    }
    let chunk_root = argument(context, args.get(1).copied());
    let chunk = response_chunk_bytes(context, chunk_root, args.get(6).copied())?;
    let explicit = args
        .get(2)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !explicit {
        let state = context.host_mut().shared_state();
        let mut host = state.borrow_mut();
        let response = host
            .http
            .responses
            .get_mut(&id)
            .ok_or_else(|| RootedError::host("shared HTTP response is no longer active"))?;
        if response.lifecycle.is_terminal() {
            drop(host);
            return throw_response_error(
                context,
                false,
                "ERR_STREAM_WRITE_AFTER_END",
                "write after end",
            );
        }
        response.body.push(chunk, false);
        return Ok(context.boolean(true));
    }
    let (status, status_message) = response_status(
        context,
        id,
        args.get(5).copied(),
        args.get(3).copied(),
        args.get(4).copied(),
    )?;
    response_send_date(context, id, args.get(5).copied())?;
    let state = context.host_mut().shared_state();
    let (socket, bytes) = {
        let mut host = state.borrow_mut();
        let response = host
            .http
            .responses
            .get_mut(&id)
            .ok_or_else(|| RootedError::host("shared HTTP response is no longer active"))?;
        if response.lifecycle.is_terminal() {
            drop(host);
            return throw_response_error(
                context,
                false,
                "ERR_STREAM_WRITE_AFTER_END",
                "write after end",
            );
        }
        let first_write = response.lifecycle == super::state::ResponseLifecycle::Open;
        let bytes = if first_write {
            let omit_framing = response.removed_headers.contains("content-length")
                && response.removed_headers.contains("transfer-encoding");
            if !omit_framing {
                enable_streaming_framing(
                    &mut response.headers,
                    response.use_chunked_encoding_by_default,
                );
            }
            if invalid_trailer(&response.headers) {
                drop(host);
                return throw_invalid_trailer(context);
            }
            response.lifecycle.send_headers();
            crate::modules::http_protocol::compose(
                status,
                &status_message,
                &response.headers,
                &chunk,
                &[],
                !response.http10 && !response.close_after_response,
                response.removed_headers.contains("connection"),
                omit_framing,
                response.http10,
                response.send_date,
                std::time::SystemTime::now(),
            )
        } else if is_chunked(&response.headers) {
            crate::modules::http_protocol::chunk_frame(&chunk)
        } else {
            chunk
        };
        (response.socket, bytes)
    };
    net_shared_vm::write(
        &mut context.host_mut().shared_state().borrow_mut().tcp,
        socket,
        &bytes,
    )
    .map_err(RootedError::host)?;
    Ok(context.boolean(true))
}

pub(crate) fn response_write_head(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let lifecycle = context
        .host_mut()
        .shared_state()
        .borrow()
        .http
        .responses
        .get(&id)
        .map(|response| response.lifecycle);
    if lifecycle.is_some_and(|state| state != super::state::ResponseLifecycle::Open) {
        return throw_response_error(
            context,
            false,
            "ERR_HTTP_HEADERS_SENT",
            "Cannot write headers after they are sent to the client",
        );
    }
    let status_root = argument(context, args.get(1).copied());
    let message_root = argument(context, args.get(2).copied());
    let response_root = argument(context, args.get(5).copied());
    let status = crate::modules::http_status_shared_vm::prepare(
        context,
        response_root,
        status_root,
        crate::modules::http_status_shared_vm::Reason::WriteHead(message_root),
    )?;
    let invalid_headers = args
        .get(4)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if invalid_headers {
        return throw_response_error(
            context,
            true,
            "ERR_INVALID_ARG_VALUE",
            "The argument 'headers' is invalid",
        );
    }
    let entry_root = argument(context, args.get(3).copied());
    let relaxed = if let Some(response) = args.get(5).copied() {
        let key = context.string_rooted("_httpValidation");
        let mode = context.get_property_rooted(response, key)?;
        matches!(
            context.to_string(mode).ok().as_deref(),
            Some("relaxed" | "insecure")
        )
    } else {
        false
    };
    let entries = write_head_entries(context, entry_root, relaxed)?;
    let status_message =
        crate::modules::http_status_shared_vm::encode(context, response_root, status)?;

    response_send_date(context, id, args.get(5).copied())?;
    let state = context.host_mut().shared_state();
    let (socket, bytes) = {
        let mut host = state.borrow_mut();
        let response = host
            .http
            .responses
            .get_mut(&id)
            .ok_or_else(|| RootedError::host("shared HTTP response is no longer active"))?;
        let mut replaced = Vec::<String>::new();
        for (name, values) in entries {
            response.removed_headers.remove(&name.to_ascii_lowercase());
            if !replaced
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&name))
            {
                response
                    .headers
                    .retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
                replaced.push(name.clone());
            }
            let values = limit_header_values(&name, values);
            response
                .headers
                .extend(values.into_iter().map(|value| (name.clone(), value)));
        }
        let omit_framing = response.removed_headers.contains("content-length")
            && response.removed_headers.contains("transfer-encoding");
        if !omit_framing {
            enable_streaming_framing(
                &mut response.headers,
                response.use_chunked_encoding_by_default,
            );
        }
        normalize_no_body_response_headers(status, &mut response.headers);
        if invalid_trailer(&response.headers) {
            drop(host);
            return throw_invalid_trailer(context);
        }
        response.lifecycle.send_headers();
        let bytes = crate::modules::http_protocol::compose(
            status,
            &status_message,
            &response.headers,
            &[],
            &[],
            !response.http10 && !response.close_after_response,
            response.removed_headers.contains("connection"),
            omit_framing,
            response.http10,
            response.send_date,
            std::time::SystemTime::now(),
        );
        (response.socket, bytes)
    };
    net_shared_vm::write(
        &mut context.host_mut().shared_state().borrow_mut().tcp,
        socket,
        &bytes,
    )
    .map_err(RootedError::host)?;
    Ok(context.undefined())
}

pub(crate) fn response_finish(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let trailers = if let Some(response_root) = args.get(4).copied() {
        let key = context.string_rooted("__quenchTrailerEntries");
        let trailer_entries = context.get_property_rooted(response_root, key)?;
        context.release_root(key);
        let trailers = super::client::read_headers(context, Some(trailer_entries), false);
        context.release_root(trailer_entries);
        trailers?
    } else {
        Vec::new()
    };
    let (status, status_message) = response_status(
        context,
        id,
        args.get(4).copied(),
        args.get(1).copied(),
        args.get(2).copied(),
    )?;
    response_send_date(context, id, args.get(4).copied())?;
    let response = {
        let state = context.host_mut().shared_state();
        let mut host = state.borrow_mut();
        let Some(response) = host.http.responses.get_mut(&id) else {
            // A peer close may retire the host-side response between an
            // end() call and a later idempotent end() from Node middleware.
            drop(host);
            return Ok(context.undefined());
        };
        let headers_sent = response.lifecycle == super::state::ResponseLifecycle::HeadersSent;
        if !headers_sent
            && response
                .headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("trailer"))
        {
            enable_streaming_framing(
                &mut response.headers,
                response.use_chunked_encoding_by_default,
            );
            if invalid_trailer(&response.headers) {
                drop(host);
                return throw_invalid_trailer(context);
            }
        }
        if !response.lifecycle.end() {
            None
        } else {
            Some((
                response.socket,
                response.async_id,
                status,
                status_message.clone(),
                response.headers.clone(),
                headers_sent,
                response.body.bytes(),
                is_chunked(&response.headers),
                response.send_date,
                response.http10,
                response.close_after_response,
                response.removed_headers.clone(),
            ))
        }
    };
    let Some((
        socket,
        async_id,
        status,
        status_message,
        headers,
        headers_sent,
        body,
        chunked,
        send_date,
        http10,
        request_closes,
        removed_headers,
    )) = response
    else {
        return Ok(context.undefined());
    };
    let (server_closing, peer_ended, server_root) = {
        let host = context.host_mut().shared_state();
        let host = host.borrow();
        host.http
            .connections
            .get(&socket)
            .and_then(|connection| {
                host.http.servers.get(&connection.server).map(|server| {
                    (server.closing, connection.peer_ended, Some(server.root))
                })
            })
            .unwrap_or((false, false, None))
    };
    let allow_half_open = if let Some(server_root) = server_root {
        let property = context.string_rooted("httpAllowHalfOpen");
        let value = context.get_property_rooted(server_root, property)?;
        context.release_root(property);
        let allow = context.rooted_value(value).and_then(Value::as_bool).unwrap_or(false);
        context.release_root(value);
        allow
    } else {
        false
    };
    let close_after_response = request_closes
        || server_closing
        || (peer_ended && !allow_half_open)
        || (removed_headers.contains("content-length")
            && removed_headers.contains("transfer-encoding")
            && !headers.iter().any(|(name, _)| {
                name.eq_ignore_ascii_case("content-length")
                    || name.eq_ignore_ascii_case("transfer-encoding")
            }))
        || (http10
            && !chunked
            && !headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("content-length")))
        || headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("connection") && value.eq_ignore_ascii_case("close")
        });
    let bytes = if headers_sent {
        if chunked {
            let mut framed = crate::modules::http_protocol::chunk_frame(&body);
            framed.extend_from_slice(&crate::modules::http_protocol::chunk_terminator(&trailers));
            framed
        } else {
            body
        }
    } else {
        let chunked = is_chunked(&headers);
        let mut framed = crate::modules::http_protocol::compose(
            status,
            &status_message,
            &headers,
            &body,
            &[],
            !http10 && !close_after_response,
            removed_headers.contains("connection"),
            removed_headers.contains("content-length")
                && removed_headers.contains("transfer-encoding")
                && !headers.iter().any(|(name, _)| {
                    name.eq_ignore_ascii_case("content-length")
                        || name.eq_ignore_ascii_case("transfer-encoding")
                }),
            http10,
            send_date,
            std::time::SystemTime::now(),
        );
        if chunked {
            framed.extend_from_slice(&crate::modules::http_protocol::chunk_terminator(&trailers));
        }
        framed
    };
    if !bytes.is_empty() {
        net_shared_vm::write(
            &mut context.host_mut().shared_state().borrow_mut().tcp,
            socket,
            &bytes,
        )
        .map_err(RootedError::host)?;
    }
    if close_after_response {
        net_shared_vm::end(
            &mut context.host_mut().shared_state().borrow_mut().tcp,
            socket,
            false,
        )
        .map_err(RootedError::host)?;
    } else {
        let shared_state = context.host_mut().shared_state();
        let mut host = shared_state.borrow_mut();
        let buffered = if let Some(connection) = host.http.connections.get_mut(&socket) {
            let consumed = connection.request_bytes.min(connection.received.len());
            if consumed > 0 {
                connection.received.drain(..consumed);
            }
            connection.request_bytes = 0;
            connection.request_dispatched = false;
            connection.headers_started = None;
            connection.headers_timeout_sent = false;
            connection.request_started = None;
            connection.request_timeout_sent = false;
            let buffered = !connection.received.is_empty();
            if buffered {
                connection.headers_started = Some(std::time::Instant::now());
                connection.request_started = Some(std::time::Instant::now());
            }
            buffered
        } else {
            false
        };
        if buffered {
            net_shared_vm::queue_http_data(&mut host.tcp, socket);
        }
    }
    if let (Some(request), Some(response), Some(server), Some(socket)) = (
        args.get(3).copied(),
        args.get(4).copied(),
        args.get(5).copied(),
        args.get(6).copied(),
    ) {
        let message = context.object_rooted()?;
        set(context, message, "request", request)?;
        set(context, message, "response", response)?;
        set(context, message, "server", server)?;
        set(context, message, "socket", socket)?;
        let shared_state = context.host_mut().shared_state();
        let result = {
            let _scope =
                crate::modules::async_hooks_shared_vm::enter_context(&shared_state, async_id);
            crate::modules::diagnostics_channel_shared_vm::publish_named(
                context,
                "http.server.response.finish",
                message,
            )
        };
        for root in
            crate::modules::async_hooks_shared_vm::take_context_stores(&shared_state, async_id)
        {
            context.release_root(root);
        }
        result?;
    }
    Ok(context.undefined())
}

pub(crate) fn response_destroy(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let shared_state = context.host_mut().shared_state();
    let mut host = shared_state.borrow_mut();
    let (socket, async_id) = {
        let Some(response) = host.http.responses.remove(&id) else {
            return Ok(context.undefined());
        };
        let mut response = response;
        if !response.lifecycle.destroy() {
            host.http.responses.insert(id, response);
            return Ok(context.undefined());
        }
        (response.socket, response.async_id)
    };
    let server_id = host
        .http
        .connections
        .remove(&socket)
        .map(|connection| connection.server);
    if let Some(server_id) = server_id {
        if let Some(server) = host.http.servers.get_mut(&server_id) {
            server.connections.remove(&socket);
        }
    }
    net_shared_vm::close_socket(&mut host.tcp, socket);
    drop(host);
    for root in crate::modules::async_hooks_shared_vm::take_context_stores(&shared_state, async_id)
    {
        context.release_root(root);
    }
    Ok(context.undefined())
}

fn enable_streaming_framing(
    headers: &mut Vec<(String, String)>,
    use_chunked_encoding_by_default: bool,
) {
    let has_length = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-length"));
    let has_transfer_encoding = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding"));
    if use_chunked_encoding_by_default && !has_length && !has_transfer_encoding {
        headers.push(("Transfer-Encoding".to_owned(), "chunked".to_owned()));
    }
}

fn normalize_no_body_response_headers(status: u16, headers: &mut Vec<(String, String)>) {
    if !matches!(status, 204 | 304) {
        return;
    }

    // Node rejects transfer framing for bodyless responses. If the caller
    // explicitly requested chunked framing, close the connection after the
    // response rather than leaving the peer waiting for a chunk terminator.
    let had_transfer_encoding = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding"));
    if !had_transfer_encoding {
        return;
    }
    headers.retain(|(name, _)| !name.eq_ignore_ascii_case("transfer-encoding"));
    headers.retain(|(name, _)| !name.eq_ignore_ascii_case("connection"));
    headers.push(("Connection".to_owned(), "close".to_owned()));
}

fn is_chunked(headers: &[(String, String)]) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("transfer-encoding") && value.eq_ignore_ascii_case("chunked")
    })
}

fn invalid_trailer(headers: &[(String, String)]) -> bool {
    headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("trailer"))
        && !is_chunked(headers)
}

fn throw_invalid_trailer<T>(context: &mut Context<'_>) -> Result<T, RootedError> {
    throw_response_error(
        context,
        false,
        "ERR_HTTP_TRAILER_INVALID",
        "Trailers are invalid with this transfer encoding",
    )
}

fn function_data_id(context: &mut Context<'_>) -> Result<u64, RootedError> {
    let data = context.host_function_data()?;
    context
        .rooted_value(data)
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && *id >= 0.0)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("invalid shared HTTP operation identifier"))
}

fn argument_id(context: &mut Context<'_>, root: Option<RootId>) -> Result<u64, RootedError> {
    root.and_then(|root| context.rooted_value(root))
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && *id >= 0.0)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("invalid shared HTTP response identifier"))
}

fn argument(context: &mut Context<'_>, root: Option<RootId>) -> RootId {
    root.unwrap_or_else(|| context.undefined())
}

fn header_name(context: &mut Context<'_>, root: RootId) -> Result<String, RootedError> {
    let name = context.string_text(root)?;
    let Some(name) = name else {
        let display = context.to_string(root)?;
        return throw_header_issue(
            context,
            crate::modules::http_protocol::HeaderIssue::InvalidName(display),
        );
    };
    if name.is_empty()
        || !name
            .chars()
            .all(crate::modules::http_protocol::is_http_token_char)
    {
        return throw_header_issue(
            context,
            crate::modules::http_protocol::HeaderIssue::InvalidName(name),
        );
    }
    Ok(name)
}

fn header_values(
    context: &mut Context<'_>,
    root: RootId,
    multiple: bool,
    name: &str,
    relaxed: bool,
) -> Result<Vec<String>, RootedError> {
    let values = if multiple {
        let length_key = context.string_rooted("length");
        let length = context.get_property_rooted(root, length_key)?;
        let length = context
            .rooted_value(length)
            .and_then(Value::as_number)
            .filter(|length| length.is_finite() && *length >= 0.0)
            .unwrap_or(0.0) as usize;
        let mut values = Vec::with_capacity(length);
        for index in 0..length {
            let item = array_item(context, root, index)?;
            values.push(header_value(context, item, name, relaxed)?);
        }
        values
    } else {
        vec![header_value(context, root, name, relaxed)?]
    };
    Ok(values)
}

fn header_value(
    context: &mut Context<'_>,
    root: RootId,
    name: &str,
    relaxed: bool,
) -> Result<String, RootedError> {
    if context.rooted_value(root).is_some_and(Value::is_undefined) {
        let issue = crate::modules::http_protocol::HeaderIssue::MissingValue(name.to_owned());
        return throw_header_issue(context, issue);
    }
    let value = context.to_string(root)?;
    let valid = if relaxed {
        value.chars().all(|character| {
            let code = character as u32;
            code <= 0xff && !matches!(code, 0x00 | 0x0a | 0x0d)
        })
    } else {
        crate::modules::http_protocol::valid_header_value(&value)
    };
    if !valid {
        let issue = crate::modules::http_protocol::HeaderIssue::InvalidContent(name.to_owned());
        return throw_header_issue(context, issue);
    }
    Ok(value)
}

fn write_head_entries(
    context: &mut Context<'_>,
    root: RootId,
    relaxed: bool,
) -> Result<Vec<(String, Vec<String>)>, RootedError> {
    let length_key = context.string_rooted("length");
    let length = context.get_property_rooted(root, length_key)?;
    let length = context
        .rooted_value(length)
        .and_then(Value::as_number)
        .filter(|length| length.is_finite() && *length >= 0.0)
        .unwrap_or(0.0) as usize;
    if !length.is_multiple_of(3) {
        return Err(RootedError::host(
            "shared HTTP header entries are malformed",
        ));
    }
    let mut result = Vec::with_capacity(length / 3);
    for index in (0..length).step_by(3) {
        let name_root = array_item(context, root, index)?;
        let name = header_name(context, name_root)?;
        let values_root = array_item(context, root, index + 1)?;
        let multiple_root = array_item(context, root, index + 2)?;
        let multiple = context
            .rooted_value(multiple_root)
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let values = header_values(context, values_root, multiple, &name, relaxed)?;
        result.push((name, values));
    }
    Ok(result)
}

fn array_item(
    context: &mut Context<'_>,
    array: RootId,
    index: usize,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(&index.to_string());
    context.get_property_rooted(array, key)
}

fn limit_header_values(name: &str, mut values: Vec<String>) -> Vec<String> {
    if crate::modules::http_protocol::NON_REPEATABLE_HEADERS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
    {
        values.truncate(1);
    }
    values
}

fn coded_error_rooted(
    context: &mut Context<'_>,
    type_error: bool,
    code: &str,
    message: &str,
) -> Result<RootId, RootedError> {
    let error = if type_error {
        context.type_error_rooted(message)?
    } else {
        context.error_rooted(message)?
    };
    let code_value = context.string_rooted(code);
    set(context, error, "code", code_value)?;
    Ok(error)
}

fn throw_header_issue(
    context: &mut Context<'_>,
    issue: crate::modules::http_protocol::HeaderIssue,
) -> Result<String, RootedError> {
    let error = coded_error_rooted(context, true, issue.code(), &issue.message())?;
    Err(context.throw(error))
}

fn throw_response_error<T>(
    context: &mut Context<'_>,
    type_error: bool,
    code: &str,
    message: &str,
) -> Result<T, RootedError> {
    let error = coded_error_rooted(context, type_error, code, message)?;
    Err(context.throw(error))
}

fn set(
    context: &mut Context<'_>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!(
            "cannot install shared http property {name}"
        )))
    }
}

pub(crate) fn validate_header_name(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let name = argument(context, args.first().copied());
    let text = context.string_text(name)?;
    if text.as_ref().is_some_and(|name| {
        !name.is_empty()
            && name
                .chars()
                .all(crate::modules::http_protocol::is_http_token_char)
    }) {
        return Ok(context.undefined());
    }
    let label = argument(context, args.get(1).copied());
    let label = if context.truthy_rooted(label)? {
        label
    } else {
        context.string_rooted("Header name")
    };
    formatted_header_error(
        context,
        "ERR_INVALID_HTTP_TOKEN",
        "%s must be a valid HTTP token [\"%s\"]",
        &[label, name],
    )
}

pub(crate) fn validate_header_value(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let name = argument(context, args.first().copied());
    let value = argument(context, args.get(1).copied());
    if context.rooted_value(value).is_some_and(Value::is_undefined) {
        return formatted_header_error(
            context,
            "ERR_HTTP_INVALID_HEADER_VALUE",
            "Invalid value \"%s\" for header \"%s\"",
            &[value, name],
        );
    }
    let lenient = argument(context, args.get(2).copied());
    let lenient = context.truthy_rooted(lenient)?;
    let units = context.to_string_code_units(value)?;
    if !units
        .iter()
        .all(|unit| crate::modules::http_protocol::valid_header_unit(*unit, lenient))
    {
        let mut message: Vec<u16> = "Invalid character in header content"
            .encode_utf16()
            .collect();
        if !context.rooted_value(name).is_some_and(Value::is_undefined) {
            // ERR_INVALID_CHAR uses template-string coercion rather than %s.
            message.extend(" [\"".encode_utf16());
            message.extend(context.to_string_code_units(name)?);
            message.extend("\"]".encode_utf16());
        }
        let message = context.string_units_rooted(&message);
        return throw_header_message(context, "ERR_INVALID_CHAR", message);
    }
    Ok(context.undefined())
}

fn formatted_header_error(
    context: &mut Context<'_>,
    code: &str,
    template: &str,
    arguments: &[RootId],
) -> Result<RootId, RootedError> {
    let formatters = crate::modules::util_shared_vm::internal_formatters(context)?;
    let key = context.string_rooted("format");
    let format = context.get_property_rooted(formatters, key)?;
    let template = context.string_rooted(template);
    let mut arguments_with_template = Vec::with_capacity(arguments.len() + 1);
    arguments_with_template.push(template);
    arguments_with_template.extend_from_slice(arguments);
    let receiver = context.undefined();
    let message = context.call_rooted(format, receiver, &arguments_with_template)?;
    throw_header_message(context, code, message)
}

// Node has already formatted this message. Keep that original VM string,
// including lone surrogates, as the ordinary Error's non-enumerable message.
fn throw_header_message(
    context: &mut Context<'_>,
    code: &str,
    message: RootId,
) -> Result<RootId, RootedError> {
    let error = coded_error_rooted(context, true, code, "")?;
    set(context, error, "message", message)?;
    Err(context.throw(error))
}
