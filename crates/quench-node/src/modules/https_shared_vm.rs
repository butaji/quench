//! HTTPS public API composition over Quench's HTTP and TLS implementations.
//!
//! The public HTTPS surface shares Agent/request primitives with HTTP while
//! configuring its secure defaults and server transport through TLS.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const FACTORY: &str = quench_js_check::checked_js!(
    r#"(http, tls, connectionIntervalSymbol, net) => {
      const EventEmitter = Object.getPrototypeOf(Object.getPrototypeOf(http.Agent.prototype)).constructor;
      function Agent(options) {
        if (!(this instanceof Agent)) return new Agent(options);
        const opts = options == null ? {} : options;
        const agent = Reflect.construct(http.Agent, [opts], new.target || Agent);
        agent.defaultPort = 443;
        agent.protocol = 'https:';
        agent.maxCachedSessions = opts.maxCachedSessions === undefined ? 100 : opts.maxCachedSessions;
        // Node exposes this cache object, including its ordinary Object
        // prototype, through https.Agent._sessionCache.
        agent._sessionCache = { map: {}, list: [] };
        return agent;
      }

      Agent.prototype = Object.create(http.Agent.prototype);
      Object.defineProperty(Agent.prototype, 'constructor', { value: Agent, writable: true, configurable: true });
      Object.setPrototypeOf(Agent, http.Agent);
      Agent.prototype.getName = function getName(options = {}) {
        let name = http.Agent.prototype.getName.call(this, options);
        for (const key of ['ca', 'cert', 'clientCertEngine', 'ciphers', 'key'])
          name += `:${options[key] || ''}`;
        const pfx = options.pfx;
        let pfxKey = '';
        if (Array.isArray(pfx)) {
          for (const value of pfx) {
            const raw = value?.buf || value;
            const passphrase = value?.passphrase || options.passphrase;
            pfxKey += `:${raw}:${passphrase || ''}`;
          }
        } else if (pfx) pfxKey = pfx;
        name += `:${pfxKey}`;
        name += `:${options.rejectUnauthorized === undefined ? '' : options.rejectUnauthorized}`;
        name += `:${options.servername && options.servername !== options.host ? options.servername : ''}`;
        for (const key of ['minVersion', 'maxVersion', 'secureProtocol', 'crl'])
          name += `:${options[key] || ''}`;
        name += `:${options.honorCipherOrder === undefined ? '' : options.honorCipherOrder}`;
        for (const key of ['ecdhCurve', 'dhparam']) name += `:${options[key] || ''}`;
        name += `:${options.secureOptions === undefined ? '' : options.secureOptions}`;
        name += `:${options.sessionIdContext || ''}`;
        name += `:${options.sigalgs === undefined ? '' : JSON.stringify(options.sigalgs)}`;
        name += `:${options.privateKeyIdentifier || ''}:${options.privateKeyEngine || ''}`;
        return name;
      };
      Agent.prototype.createConnection = function createConnection(...args) {
        // https.Agent.createConnection supports the same port/host/options
        // overloads as tls.connect(), in addition to the common options form.
        let options;
        if (args[0] !== null && typeof args[0] === 'object') {
          options = args[0];
        } else if (args[1] !== null && typeof args[1] === 'object') {
          options = { ...args[1] };
        } else if (args[2] === null || typeof args[2] !== 'object') {
          options = {};
        } else {
          options = { ...args[2] };
        }
        if (typeof args[0] === 'number') options.port = args[0];
        if (typeof args[1] === 'string') options.host = args[1];
        const callback = typeof args[args.length - 1] === 'function'
          ? args[args.length - 1]
          : undefined;
        const key = options._agentKey || this.getName(options);
        const cacheSession = !options._quenchPerRequestCheckServerIdentity &&
          this.maxCachedSessions !== 0;
        const cached = cacheSession && this._sessionCache.map[key];
        const connectionOptions = cached ? { session: cached, ...options } : { ...options };
        delete connectionOptions._agentKey;
        delete connectionOptions._quenchPerRequestCheckServerIdentity;
        const socket = callback === undefined
          ? tls.connect(connectionOptions)
          : tls.connect(connectionOptions, callback);
        if (cacheSession) {
          const captureSession = () => {
            const session = socket.getSession();
            if (session && session.length) this._cacheSession(key, session);
          };
          socket.once('secureConnect', captureSession);
          // TLS 1.3 tickets may arrive after secureConnect; refreshed session
          // data is visible after subsequent encrypted reads.
          socket.on('data', captureSession);
          socket.once('close', (hadError) => {
            if (hadError) this._evictSession(key);
          });
        }
        return socket;
      };
      Agent.prototype._getSession = function _getSession(key) {
        return this._sessionCache.map[key];
      };
      Agent.prototype._cacheSession = function _cacheSession(key, session) {
        if (this.maxCachedSessions === 0) return;
        if (Object.hasOwn(this._sessionCache.map, key)) {
          this._sessionCache.map[key] = session;
          return;
        }
        if (this._sessionCache.list.length >= this.maxCachedSessions) {
          const oldest = this._sessionCache.list.shift();
          delete this._sessionCache.map[oldest];
        }
        this._sessionCache.list.push(key);
        this._sessionCache.map[key] = session;
      };
      Agent.prototype._evictSession = function _evictSession(key) {
        const index = this._sessionCache.list.indexOf(key);
        if (index < 0) return;
        this._sessionCache.list.splice(index, 1);
        delete this._sessionCache.map[key];
      };
      const destroyAgent = Agent.prototype.destroy;
      Agent.prototype.destroy = function destroy() {
        for (const pool of [...Object.values(this.sockets), ...Object.values(this.freeSockets)]) {
          for (const socket of pool) socket.destroy?.();
        }
        this.sockets = Object.create(null);
        this.freeSockets = Object.create(null);
        this.requests = Object.create(null);
        return destroyAgent.call(this);
      };

      function installHttpParser(server) {
        server._quenchTlsSockets = new Set();
        server.on('secureConnection', (socket) => {
          server._quenchTlsSockets.add(socket);
          socket.once('close', () => server._quenchTlsSockets.delete(socket));
          socket._quenchArmTimeout = () => {
            if (socket._quenchTimeoutTimer !== undefined) clearTimeout(socket._quenchTimeoutTimer);
            if (server.timeout > 0) {
              socket._quenchTimeoutTimer = setTimeout(() => {
                socket._quenchTimeoutTimer = undefined;
                server.emit('timeout', socket);
              }, server.timeout);
            }
          };
          socket._quenchArmTimeout();
          socket.on('data', socket._quenchArmTimeout);
          socket.once('close', () => {
            if (socket._quenchTimeoutTimer !== undefined) clearTimeout(socket._quenchTimeoutTimer);
          });
          let pending = Buffer.alloc(0);
          socket.on('data', (chunk) => {
            pending = Buffer.concat([pending, chunk]);
            for (;;) {
              const boundary = pending.indexOf('\r\n\r\n');
              if (boundary < 0) return;
              const maxHeaderSize = server.maxHeaderSize ?? http.maxHeaderSize ?? 16 * 1024;
              if (boundary + 4 > maxHeaderSize) {
                const error = new Error('Parse Error');
                error.code = 'HPE_HEADER_OVERFLOW';
                server.emit('clientError', error, socket);
                socket.destroy();
                return;
              }
              const lines = pending.subarray(0, boundary).toString('latin1').split('\r\n');
              const [method = 'GET', url = '/', protocolVersion = 'HTTP/1.1'] = lines.shift().split(' ');
              const httpVersion = protocolVersion.startsWith('HTTP/')
                ? protocolVersion.slice('HTTP/'.length)
                : protocolVersion;
              const headers = Object.create(null);
              const rawHeaders = [];
              for (const line of lines) {
                const colon = line.indexOf(':');
                if (colon < 1) continue;
                const name = line.slice(0, colon).trim();
                const value = line.slice(colon + 1).trim();
                rawHeaders.push(name, value);
                const key = name.toLowerCase();
                headers[key] = headers[key] === undefined ? value : `${headers[key]}, ${value}`;
              }
              const bodyLength = Math.max(0, Number.parseInt(headers['content-length'] || '0', 10) || 0);
              const end = boundary + 4 + bodyLength;
              if (pending.length < end) return;
              const body = pending.subarray(boundary + 4, end);
              pending = pending.subarray(end);
              // Node rejects HTTP/1.1 requests without Host before invoking
              // the application listener. `setHost: false` on a client
              // request therefore receives a 400 while remaining invisible
              // to the HTTPS server's request callback.
              if (httpVersion === '1.1' && server.requireHostHeader && !Object.hasOwn(headers, 'host')) {
                socket.end('HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n');
                return;
              }
              const request = Object.assign(new EventEmitter(), {
                method, url, httpVersion, headers, rawHeaders, socket, connection: socket,
                complete: true, aborted: false, readable: true,
                resume() { return this; },
                setTimeout(milliseconds, callback) {
                  if (typeof callback === 'function') this.once('timeout', callback);
                  const timer = setTimeout(() => this.emit('timeout', socket), milliseconds);
                  socket.once('close', () => clearTimeout(timer));
                  return this;
                },
                getUserAgent() { return this.headers['user-agent']; },
              });
              Object.setPrototypeOf(request, (server.IncomingMessage || http.IncomingMessage).prototype);
              let statusCode = 200;
              let statusMessage;
              const responseHeaders = Object.create(null);
              const chunks = [];
              const response = {
                req: request, socket, connection: socket,
                statusCode: 200, headersSent: false,
                setTimeout(milliseconds, callback) {
                  if (typeof callback === 'function') this.once('timeout', callback);
                  const timer = setTimeout(() => this.emit('timeout', socket), milliseconds);
                  socket.once('close', () => clearTimeout(timer));
                  return this;
                },
                setHeader(name, value) { responseHeaders[String(name).toLowerCase()] = [String(name), String(value)]; return this; },
                getHeader(name) { return responseHeaders[String(name).toLowerCase()]?.[1]; },
                removeHeader(name) { delete responseHeaders[String(name).toLowerCase()]; },
                writeHead(code, message, headersArg) {
                  statusCode = code;
                  if (typeof message === 'string') statusMessage = message;
                  else headersArg = message;
                  if (headersArg && typeof headersArg === 'object')
                    for (const [name, value] of Object.entries(headersArg)) this.setHeader(name, value);
                  return this;
                },
                status(code) { return this.writeHead(code); },
                write(chunk, encoding, callback) {
                  chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk, encoding));
                  if (typeof callback === 'function') process.nextTick(callback);
                  return true;
                },
                end(chunk, encoding, callback) {
                  if (chunk !== undefined && typeof chunk !== 'function') this.write(chunk, encoding);
                  if (typeof chunk === 'function') callback = chunk;
                  else if (typeof encoding === 'function') callback = encoding;
                  const payload = Buffer.concat(chunks);
                  if (responseHeaders['content-length'] === undefined && statusCode !== 204 && statusCode !== 304)
                    this.setHeader('Content-Length', payload.length);
                  const phrase = statusMessage || ({ 200: 'OK', 201: 'Created', 204: 'No Content', 301: 'Moved Permanently', 302: 'Found', 400: 'Bad Request', 404: 'Not Found', 500: 'Internal Server Error' }[statusCode] || 'OK');
                  let head = `HTTP/1.1 ${statusCode} ${phrase}\r\n`;
                  this.statusCode = statusCode;
                  this.headersSent = true;
                  for (const [name, value] of Object.values(responseHeaders)) head += `${name}: ${value}\r\n`;
                  socket.write(Buffer.concat([Buffer.from(`${head}\r\n`), payload]));
                  if (headers.connection === 'close' || request.headers.connection === 'close') socket.end();
                  if (typeof callback === 'function') process.nextTick(callback);
                  return this;
                },
              };
              Object.setPrototypeOf(response, (server.ServerResponse || http.ServerResponse).prototype);
              server.emit('request', request, response);
              process.nextTick(() => { if (body.length) request.emit('data', body); request.emit('end'); });
            }
          });
        });
      }

      function Server(options, requestListener) {
        if (!(this instanceof Server)) return new Server(options, requestListener);
        if (typeof options === 'function') {
          requestListener = options;
          options = undefined;
        } else if (options !== undefined && options !== null &&
                   (typeof options !== 'object' || Array.isArray(options))) {
          let received;
          if (typeof options === 'string') received = `type string ('${options}')`;
          else if (typeof options === 'number' || typeof options === 'boolean') {
            received = `type ${typeof options} (${String(options)})`;
          } else if (typeof options === 'symbol') received = `type symbol (${String(options)})`;
          else if (typeof options === 'bigint') received = `type bigint (${options}n)`;
          else if (Array.isArray(options)) received = 'an instance of Array';
          else received = `type ${typeof options}`;
          const error = new TypeError(
            `The "options" argument must be of type object. Received ${received}`
          );
          error.code = 'ERR_INVALID_ARG_TYPE';
          throw error;
        }
        const opts = options == null ? {} : { ...options };
        if (opts.ALPNProtocols === undefined && opts.ALPNCallback === undefined)
          opts.ALPNProtocols = ['http/1.1'];
        const server = Reflect.construct(tls.Server, [opts], new.target || Server);
        server.httpAllowHalfOpen = false;
        server.timeout = 0;
        // The TLS server constructor does not inherit HTTP Server's public
        // timeout defaults, so project those options onto the HTTPS server.
        server.requestTimeout = opts.requestTimeout === undefined ? 300000 : opts.requestTimeout;
        server.headersTimeout = opts.headersTimeout === undefined
          ? Math.min(60000, server.requestTimeout)
          : opts.headersTimeout;
        server.maxHeadersCount = null;
        server.maxHeaderSize = opts.maxHeaderSize ?? http.maxHeaderSize ?? 16 * 1024;
        server.requireHostHeader = opts.requireHostHeader === undefined ? true : opts.requireHostHeader;
        server.ServerResponse = opts.ServerResponse || http.ServerResponse;
        server.IncomingMessage = opts.IncomingMessage || http.IncomingMessage;
        installHttpParser(server);
        server[connectionIntervalSymbol] = { _destroyed: false };
        const closeServer = server.close.bind(server);
        server.close = function close(callback) {
          server[connectionIntervalSymbol]._destroyed = true;
          return closeServer(callback);
        };
        if (requestListener !== undefined)
          server.on('request', requestListener);
        return server;
      }
      Server.prototype = Object.create(tls.Server.prototype);
      Object.defineProperty(Server.prototype, 'constructor', { value: Server, writable: true, configurable: true });
      Object.setPrototypeOf(Server, tls.Server);
      Server.prototype.closeAllConnections = function closeAllConnections() {
        for (const socket of this._quenchTlsSockets || []) socket.destroy();
      };
      Server.prototype.closeIdleConnections = function closeIdleConnections() { return this.closeAllConnections(); };
      Server.prototype.setTimeout = function setTimeout(milliseconds, callback) {
        this.timeout = milliseconds;
        if (typeof callback === 'function') this.on('timeout', callback);
        for (const socket of this._quenchTlsSockets || [])
          socket._quenchArmTimeout?.();
        return this;
      };
      Server.prototype[Symbol.asyncDispose] = function asyncDispose() {
        return new Promise((resolve, reject) => {
          this.close((error) => error ? reject(error) : resolve());
        });
      };

      function request(input, options, callback) {
        if (typeof options === 'function') { callback = options; options = undefined; }
        const supplied = input && typeof input === 'object' ? input : {};
        const href = typeof input === 'string' ? input : supplied.href;
        let parsed = {};
        if (typeof href === 'string') {
          const url = new URL(href);
          parsed = { hostname: url.hostname, port: Number(url.port || 443), path: `${url.pathname || '/'}${url.search}` };
        }
        const opts = { ...parsed, ...supplied, ...(options || {}), protocol: 'https:' };
        if (opts.rejectUnauthorized === undefined && process.env.NODE_TLS_REJECT_UNAUTHORIZED === '0')
          opts.rejectUnauthorized = false;
        opts.hostname ||= opts.host || 'localhost';
        opts.port ||= 443;
        opts.method ||= 'GET';
        opts.path ||= '/';
        const outgoing = new EventEmitter();
        outgoing.agent = opts.agent === undefined ? globalAgent : opts.agent;
        outgoing.reusedSocket = false;
        outgoing.aborted = false;
        outgoing.destroyed = false;
        outgoing.writable = true;
        outgoing._body = [];
        outgoing.write = function write(chunk, encoding, done) {
          if (typeof encoding === 'function') { done = encoding; encoding = undefined; }
          const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk, encoding);
          if (sent && socket) socket.write(bytes);
          else outgoing._body.push(bytes);
          if (typeof done === 'function') process.nextTick(done);
          return true;
        };
        outgoing.end = function end(chunk, encoding, done) {
          if (typeof chunk === 'function') { done = chunk; chunk = undefined; }
          else if (typeof encoding === 'function') { done = encoding; encoding = undefined; }
          if (chunk !== undefined) outgoing.write(chunk, encoding);
          if (outgoing._ended) return outgoing;
          outgoing._ended = true;
          outgoing.writable = false;
          if (typeof done === 'function') outgoing.once('finish', done);
          if (sent) outgoing.emit('finish');
          else flush();
          return outgoing;
        };
        outgoing.setTimeout = function setTimeout(milliseconds, listener) {
          if (typeof listener === 'function') outgoing.once('timeout', listener);
          if (socket) socket.setTimeout(milliseconds, () => outgoing.emit('timeout'));
          else timeout = milliseconds;
          return outgoing;
        };
        outgoing.abort = function abort() { outgoing.aborted = true; return outgoing.destroy(); };
        outgoing.destroy = function destroy(error) {
          outgoing.destroyed = true;
          if (socket) socket.destroy(error);
          else if (error) process.nextTick(() => outgoing.emit('error', error));
          return outgoing;
        };
        outgoing.setHeader = function setHeader(name, value) {
          addHeader(name, value);
          return outgoing;
        };
        outgoing.getHeader = function getHeader(name) { return headers.get(String(name).toLowerCase())?.[1]; };
        outgoing.getHeaders = function getHeaders() {
          return Object.fromEntries([...headers].map(([name, pair]) => [name, pair[1]]));
        };
        outgoing.removeHeader = function removeHeader(name) { headers.delete(String(name).toLowerCase()); };
        outgoing.flushHeaders = function flushHeaders() { outgoing._headersFlushed = true; return outgoing; };
        outgoing.setNoDelay = function setNoDelay(value = true) { socket?.setNoDelay(value); return outgoing; };
        outgoing.setSocketKeepAlive = function setSocketKeepAlive(enable = false, delay) {
          socket?.setKeepAlive(enable, delay);
          return outgoing;
        };
        outgoing.renegotiate = function renegotiate(options, callback) {
          if (socket?.renegotiate) return socket.renegotiate(options, callback);
          if (typeof callback === 'function') process.nextTick(() => callback(new Error('TLS renegotiation is unavailable')));
          return false;
        };
        if (typeof callback === 'function') outgoing.once('response', callback);

        const headers = new Map();
        const addHeader = (name, value) => headers.set(String(name).toLowerCase(), [String(name), Array.isArray(value) ? value.join(', ') : String(value)]);
        if (Array.isArray(opts.headers)) {
          for (let index = 0; index + 1 < opts.headers.length; index += 2) addHeader(opts.headers[index], opts.headers[index + 1]);
        } else if (opts.headers && typeof opts.headers === 'object') {
          for (const [name, value] of Object.entries(opts.headers)) addHeader(name, value);
        }
        if (opts.auth && !headers.has('authorization')) addHeader('Authorization', `Basic ${Buffer.from(String(opts.auth)).toString('base64')}`);
        // Node's setHost option controls automatic Host generation. Treat
        // falsey supplied values as disabled while retaining the default-on
        // behavior when the option is omitted.
        if (!headers.has('host') && (opts.setHost === undefined || Boolean(opts.setHost))) {
          let host = String(opts.hostname || opts.host || 'localhost');
          // Match Node's Host header formatting: non-default HTTPS ports are
          // part of the authority, and IPv6 literals need brackets there.
          if (host.includes(':') && !(host.startsWith('[') && host.endsWith(']')))
            host = `[${host}]`;
          addHeader('Host', Number(opts.port) === 443 ? host : `${host}:${opts.port}`);
        }
        if (!headers.has('connection')) addHeader('Connection', 'close');

        let socket;
        let timeout;
        let responseBuffer = Buffer.alloc(0);
        let sent = false;
        const flush = () => {
          if (!socket || !socket._secureEstablished || sent) return;
          sent = true;
          const lines = [`${opts.method} ${opts.path} HTTP/1.1`];
          for (const [name, value] of headers.values()) lines.push(`${name}: ${value}`);
          socket.write(Buffer.from(`${lines.join('\r\n')}\r\n\r\n`));
          for (const body of outgoing._body) socket.write(body);
          outgoing._body.length = 0;
          if (outgoing._ended) outgoing.emit('finish');
        };
        const onSocket = (value) => {
          socket = value;
          outgoing.socket = value;
          outgoing.connection = value;
          if (timeout !== undefined) outgoing.setTimeout(timeout);
          socket.on('secureConnect', flush);
          socket.on('data', (chunk) => {
            responseBuffer = Buffer.concat([responseBuffer, chunk]);
            const boundary = responseBuffer.indexOf('\r\n\r\n');
            if (boundary < 0) return;
            // `maxHeaderSize` is a per-request client parser limit. Check the
            // complete response head before exposing a response, including
            // responses arriving over caller-supplied Duplex sockets.
            const maxHeaderSize = opts.maxHeaderSize ?? http.maxHeaderSize ?? 16 * 1024;
            if (boundary + 4 > maxHeaderSize) {
              const error = new Error('Parse Error');
              error.code = 'HPE_HEADER_OVERFLOW';
              outgoing.destroyed = true;
              socket.destroy();
              process.nextTick(() => outgoing.emit('error', error));
              return;
            }
            const lines = responseBuffer.subarray(0, boundary).toString('latin1').split('\r\n');
            const status = lines.shift().match(/^HTTP\/\d\.\d\s+(\d+)\s*(.*)$/);
            if (!status) return outgoing.destroy(new Error('Invalid HTTPS response'));
            const responseHeaders = Object.create(null);
            const rawHeaders = [];
            for (const line of lines) {
              const colon = line.indexOf(':');
              if (colon < 1) continue;
              const name = line.slice(0, colon).trim();
              const value = line.slice(colon + 1).trim();
              rawHeaders.push(name, value);
              const key = name.toLowerCase();
              responseHeaders[key] = responseHeaders[key] === undefined ? value : `${responseHeaders[key]}, ${value}`;
            }
            const length = Number.parseInt(responseHeaders['content-length'] || '0', 10) || 0;
            const start = boundary + 4;
            if (responseBuffer.length < start + length) return;
            const body = responseBuffer.subarray(start, start + length);
            responseBuffer = responseBuffer.subarray(start + length);
            const response = Object.assign(new EventEmitter(), {
              statusCode: Number(status[1]), statusMessage: status[2], httpVersion: '1.1',
              headers: responseHeaders, rawHeaders, socket, connection: socket,
              complete: true, readable: true,
              resume() { return this; }, pause() { return this; }, setEncoding() { return this; },
                setTimeout(milliseconds, callback) {
                  if (typeof callback === 'function') this.once('timeout', callback);
                  const timer = setTimeout(() => this.emit('timeout', socket), milliseconds);
                  socket.once('close', () => clearTimeout(timer));
                  return this;
                },
              get rawTrailers() { return []; },
              destroy(error) { if (error) this.emit('error', error); return this; },
            });
            outgoing.emit('response', response);
            process.nextTick(() => {
              if (body.length) response.emit('data', body);
              response.emit('end'); response.emit('close'); outgoing.emit('close');
              if (outgoing.agent?.keepAlive) {
                const name = outgoing.agent.getName(opts);
                (outgoing.agent.freeSockets[name] ||= []).push(socket);
                // Agent consumers observe the free transition on the socket
                // itself (for example, TLSSocket users waiting before issuing
                // another request), as well as on the Agent.
                socket.emit('free');
                outgoing.agent.emit('free', socket, opts);
              }
              socket.end();
            });
          });
          socket.on('error', (error) => { if (!outgoing.destroyed) outgoing.emit('error', error); });
          flush();
        };
        // https.Agent.createConnection normally receives options already
        // merged by Node's Agent.createSocket. This facade calls it directly,
        // so preserve the same request-then-agent precedence here.
        const connectionOptions = { ...opts, ...(outgoing.agent?.options || {}) };
        connectionOptions.host = connectionOptions.hostname || connectionOptions.host || opts.hostname;
        if (outgoing.agent && outgoing.agent._sessionCache) {
          connectionOptions._agentKey = outgoing.agent.getName(connectionOptions);
          connectionOptions._quenchPerRequestCheckServerIdentity =
            typeof opts.checkServerIdentity === 'function' &&
            opts.checkServerIdentity !== tls.checkServerIdentity &&
            !outgoing.agent.options?.checkServerIdentity;
        }
        const servername = opts.servername || (net.isIP(opts.hostname) ? undefined : opts.hostname);
        if (servername === undefined) delete connectionOptions.servername;
        else connectionOptions.servername = servername;
        delete connectionOptions.path;
        delete connectionOptions.method;
        delete connectionOptions.protocol;
        delete connectionOptions.headers;
        delete connectionOptions.agent;
        // Match ClientRequest's custom connection hook. In particular, callers
        // use this for in-memory Duplex sockets in the HTTP parser tests; do
        // not open the HTTPS agent's default localhost connection when it is
        // supplied.
        if (typeof opts.createConnection === 'function')
          onSocket(opts.createConnection(connectionOptions));
        else if (outgoing.agent && typeof outgoing.agent.createConnection === 'function')
          onSocket(outgoing.agent.createConnection(connectionOptions));
        else onSocket(tls.connect(connectionOptions));
        return outgoing;
      }
      function get(input, options, callback) {
        const outgoing = request(input, options, callback);
        outgoing.end();
        return outgoing;
      }

      const globalAgent = new Agent({ keepAlive: true, scheduling: 'lifo', timeout: 5000 });
      function createServer(options, requestListener) {
        return new Server(options, requestListener);
      }

      return {
        Agent,
        globalAgent,
        Server,
        createServer,
        request,
        get,
        createConnection: tls.connect,
      };
    }"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(FACTORY, "node:https/shared.js")?;
    let http = crate::modules::http_shared_vm::module(context)?;
    let tls = crate::modules::tls_shared_vm::module(context)?;
    let net = crate::host::shared_vm::commonjs::net_module(context)?;
    let connection_interval = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "_http_server.js",
        "_http_server.js",
        "kConnectionsCheckingInterval",
    )?;
    let undefined = context.undefined();
    let api = context.call_rooted(factory, undefined, &[http, tls, connection_interval, net])?;
    context.release_root(factory);
    Ok(api)
}
