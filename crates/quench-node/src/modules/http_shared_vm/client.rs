use crate::host::NodeHost;
use crate::modules::net_shared_vm;
use quench_runtime::{NativeContext, RootId, RootedError, Value};

type Context<'a> = NativeContext<'a, NodeHost>;

const REQUEST_FUNCTION: &str = r#"((Writable, Readable, create, write, setHeader, removeHeader, end, destroy, normalizeUrl, publishDiagnostics, asyncIdSymbol) => {
  const DEFAULT_HTTP_HIGH_WATER_MARK = 64 * 1024;
  class ReusedHandle {
    constructor(type, handle) { this.type = type; this.handle = handle; }
  }
  function asyncResetHandle(socket) {
    const handle = socket?._handle;
    if (handle && typeof handle.asyncReset === 'function') {
      handle.asyncReset(new ReusedHandle(handle.getProviderType(), handle));
      socket[asyncIdSymbol] = handle.getAsyncId();
    }
  }
  // Keep the returned Writable on a stable ClientRequest prototype so the
  // public `http.ClientRequest` constructor can recognize requests created by
  // both `request()` and `get()`.
  const clientRequestPrototype = Object.create(Writable.prototype);
  const headerPairs = (headers, uniqueHeaders = new Set()) => {
    const pairs = [];
    const add = (name, value) => {
      const textName = String(name);
      const key = textName.toLowerCase();
      const values = Array.isArray(value) ? value.map(String) : [String(value)];
      if (Array.isArray(value) && (key === "cookie" || uniqueHeaders.has(key))) {
        pairs.push(textName, values.join("; "));
      } else {
        for (const item of values) pairs.push(textName, item);
      }
    };
    if (Array.isArray(headers)) {
      if (headers.length && Array.isArray(headers[0])) {
        for (const [name, value] of headers) add(name, value);
      } else {
        for (let index = 0; index + 1 < headers.length; index += 2) {
          add(headers[index], headers[index + 1]);
        }
      }
    } else if (headers && typeof headers === "object") {
      for (const [name, value] of Object.entries(headers)) add(name, value);
    }
    return pairs;
  };
  const replaceHeaderPairs = (pairs, name, replacement) => {
    const key = String(name).toLowerCase();
    let first = -1;
    for (let index = 0; index < pairs.length; index += 2) {
      if (String(pairs[index]).toLowerCase() === key) { first = index; break; }
    }
    if (first === -1) first = pairs.length;
    for (let index = pairs.length - 2; index >= 0; index -= 2) {
      if (String(pairs[index]).toLowerCase() === key) pairs.splice(index, 2);
    }
    pairs.splice(first, 0, ...replacement);
  };
  function request(input = {}, overrides, callback) {
    if (typeof overrides === "function") { callback = overrides; overrides = undefined; }
    let options = input;
    if (typeof input === "string") {
      options = normalizeUrl(input);
    } else if (input && typeof input.href === "string") {
      const normalized = normalizeUrl(input.href);
      options = {
        ...normalized,
        ...input,
        hostname: normalized.hostname,
        port: normalized.port,
        path: normalized.path,
      };
    }
    if (overrides !== undefined && overrides !== null) options = { ...options, ...overrides };
    const maxHeaderSize = options?.maxHeaderSize ?? 16 * 1024;
    // Node's client option normalization ignores inherited hostname values.
    // Besides accepting null-prototype option objects, this avoids invoking a
    // poisoned Object.prototype.hostname getter while constructing a request.
    const optionHostname = options != null && Object.hasOwn(options, 'hostname')
      ? options.hostname
      : undefined;
    const timeout = options?.timeout;
    if (timeout !== undefined && typeof timeout !== 'number') {
      let received;
      if (Array.isArray(timeout)) received = 'an instance of Array';
      else if (timeout === null) received = 'null';
      else if (typeof timeout === 'string') received = `type string ('${timeout}')`;
      else if (typeof timeout === 'boolean') received = `type boolean (${timeout})`;
      else if (typeof timeout === 'symbol') received = `type symbol (${String(timeout)})`;
      else if (typeof timeout === 'bigint') received = `type bigint (${timeout}n)`;
      else if (typeof timeout === 'function') received = `function ${timeout.name}`;
      else received = `an instance of ${timeout?.constructor?.name || 'Object'}`;
      const error = new TypeError(
        `The "timeout" argument must be of type number. Received ${received}`
      );
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (timeout !== undefined && (!Number.isFinite(timeout) || timeout < 0)) {
      const received = Number.isNaN(timeout) ? 'NaN' : String(timeout);
      const error = new RangeError(
        `The value of "timeout" is out of range. It must be a non-negative finite number. Received ${received}`
      );
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    const validatePath = (value) => {
      const path = String(value);
      if (Array.from(path).some((character) => {
        const codePoint = character.codePointAt(0);
        return codePoint < 0x21 || codePoint > 0xff;
      })) {
        const error = new TypeError('Request path contains unescaped characters');
        error.code = 'ERR_UNESCAPED_CHARACTERS';
        throw error;
      }
      return path;
    };
    const agent = options?.agent;
    if (agent !== undefined && agent !== null && agent !== false &&
        !(agent && typeof agent === "object" &&
          typeof agent._quenchSharedAgentId === "number")) {
      let received;
      if (Array.isArray(agent)) received = "Received an instance of Array";
      else if (agent === null) received = "Received null";
      else if (typeof agent === "string") received = `Received type string ('${agent}')`;
      else if (typeof agent === "number" || typeof agent === "boolean") {
        received = `Received type ${typeof agent} (${String(agent)})`;
      } else if (typeof agent === "symbol") received = `Received type symbol (${String(agent)})`;
      else if (typeof agent === "function") received = "Received function ";
      else received = `Received an instance of ${agent?.constructor?.name || "Object"}`;
      const error = new TypeError(
        `The \"options.agent\" property must be one of Agent-like Object, undefined, or false. ${received}`
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const method = options?.method;
    if (method !== undefined && method !== null && typeof method !== "string") {
      let received;
      if (Array.isArray(method)) received = "an instance of Array";
      else if (typeof method === "string") received = `type string (${JSON.stringify(method)})`;
      else if (typeof method === "number" || typeof method === "boolean") {
        received = `type ${typeof method} (${String(method)})`;
      } else if (typeof method === "symbol") received = `type symbol (${String(method)})`;
      else if (typeof method === "function") received = `function ${method.name}`;
      else received = `an instance of ${method?.constructor?.name || "Object"}`;
      const error = new TypeError(`The \"options.method\" property must be of type string. Received ${received}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (method && !/^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/.test(method)) {
      const error = new TypeError(`Method must be a valid HTTP token ["${method}"]`);
      error.code = 'ERR_INVALID_HTTP_TOKEN';
      throw error;
    }
    for (const name of ['hostname', 'host']) {
      const value = name === 'hostname' ? optionHostname : options?.[name];
      if (value === undefined || value === null || typeof value === 'string') continue;
      let received;
      if (Array.isArray(value)) received = 'an instance of Array';
      else if (typeof value === 'number' || typeof value === 'boolean') {
        received = `type ${typeof value} (${String(value)})`;
      } else if (typeof value === 'symbol') received = `type symbol (${String(value)})`;
      else if (typeof value === 'function') received = 'function ';
      else received = `an instance of ${value?.constructor?.name || 'Object'}`;
      const error = new TypeError(
        `The "options.${name}" property must be of type string or one of undefined or null. Received ${received}`
      );
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (options?.insecureHTTPParser !== undefined &&
        typeof options.insecureHTTPParser !== 'boolean') {
      const value = options.insecureHTTPParser;
      let received;
      if (Array.isArray(value)) received = 'an instance of Array';
      else if (value === null) received = 'null';
      else if (typeof value === 'string') received = `type string ('${value}')`;
      else if (typeof value === 'number' || typeof value === 'boolean') {
        received = `type ${typeof value} (${String(value)})`;
      } else if (typeof value === 'symbol') received = `type symbol (${String(value)})`;
      else if (typeof value === 'function') received = 'function ';
      else received = `an instance of ${value?.constructor?.name || 'Object'}`;
      const error = new TypeError(
        `The "options.insecureHTTPParser" property must be of type boolean. Received ${received}`
      );
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    const httpValidation = options?.httpValidation;
    if (httpValidation !== undefined) {
      if (!['strict', 'relaxed', 'insecure'].includes(httpValidation)) {
        const received = typeof httpValidation === 'string'
          ? `'${httpValidation}'`
          : httpValidation === null ? 'null' : String(httpValidation);
        const error = new TypeError(
          `The property 'options.httpValidation' must be one of: 'strict', 'relaxed', 'insecure'. Received ${received}`
        );
        error.code = 'ERR_INVALID_ARG_VALUE';
        throw error;
      }
      if (options.insecureHTTPParser !== undefined) {
        const error = new TypeError(
          `The property 'options.httpValidation' cannot be used together with options.insecureHTTPParser. Received '${httpValidation}'`
        );
        error.code = 'ERR_INVALID_ARG_VALUE';
        throw error;
      }
    }
    const requestPath = validatePath(options?.path || '/');
    const optionHeaders = options?.headers;
    if (optionHeaders && typeof optionHeaders === 'object' && !Array.isArray(optionHeaders)) {
      for (const name of Object.keys(optionHeaders)) {
        const value = optionHeaders[name];
        if (String(name).toLowerCase() !== 'host' || !value || typeof value === 'string') continue;
        let received;
        if (Array.isArray(value)) received = 'an instance of Array';
        else if (typeof value === 'number' || typeof value === 'boolean') {
          received = `type ${typeof value} (${String(value)})`;
        } else if (typeof value === 'symbol') received = `type symbol (${String(value)})`;
        else if (typeof value === 'function') received = 'function ';
        else received = `an instance of ${value?.constructor?.name || 'Object'}`;
        const error = new TypeError(
          `The "options.headers.host" property must be of type string. Received ${received}`
        );
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
    }
    const uniqueHeaderNames = new Set(Array.isArray(options?.uniqueHeaders)
      ? options.uniqueHeaders.map((name) => String(name).toLowerCase())
      : []);
    const requestHeaders = headerPairs(optionHeaders, uniqueHeaderNames);
    const contentLengthIndex = requestHeaders.findIndex((value, index) =>
      index % 2 === 0 && String(value).toLowerCase() === 'content-length');
    const expectedContentLength = contentLengthIndex === -1
      ? undefined
      : Number(requestHeaders[contentLengthIndex + 1]);
    const relaxedHeaderValues = httpValidation === 'relaxed' || httpValidation === 'insecure';
    const validateHeaderValue = (name, value) => {
      const invalid = relaxedHeaderValues
        ? /[\x00\x0a\x0d]|[^\x00-\xff]/
        : /[^\t\x20-\x7e\x80-\xff]/;
      const values = Array.isArray(value) ? value : [value];
      for (const item of values) {
        if (invalid.test(String(item))) {
          const error = new TypeError(`Invalid character in header content ["${name}"]`);
          error.code = 'ERR_INVALID_CHAR';
          throw error;
        }
      }
    };
    for (let index = 0; index < requestHeaders.length; index += 2) {
      validateHeaderValue(requestHeaders[index], requestHeaders[index + 1]);
    }
    if (options?.setDefaultHeaders !== false && !Array.isArray(optionHeaders) &&
        options?.auth !== undefined &&
        !requestHeaders.some((value, index) =>
          index % 2 === 0 && String(value).toLowerCase() === "authorization")) {
      requestHeaders.push("Authorization", `Basic ${Buffer.from(String(options.auth)).toString("base64")}`);
    }
    // Without an Agent there is no connection pool to retain the socket, so
    // Node marks the request non-persistent unless the caller chose a
    // Connection header explicitly.
    if (options?.setDefaultHeaders !== false && options?.agent === false && !requestHeaders.some((value, index) =>
      index % 2 === 0 && String(value).toLowerCase() === 'connection')) {
      requestHeaders.push('Connection', 'close');
    }
    const id = create(options, requestHeaders,
      options?.setDefaultHeaders !== false, Array.isArray(optionHeaders));
    let agentPool;
    if (agent && agent.sockets && agent.freeSockets && agent.requests) {
      const name = agent.getName(options || {});
      const sockets = agent.sockets[name] || (agent.sockets[name] = []);
      const free = agent.freeSockets[name] || [];
      const requests = agent.requests[name] || [];
      let slot;
      let reused = false;
      // A socket may already have unsolicited bytes queued in the OS receive
      // buffer even though the event loop has not delivered a `data` event.
      // Check before assigning it to a new request, matching Agent's native
      // pending-read guard and preventing response queue poisoning.
      while (free.length > 0) {
        const candidate = agent.scheduling === 'fifo' ? free.shift() : free.pop();
        if (candidate?._handle?.hasPendingRead?.() || candidate?._handle?.writeQueueSize > 0) {
          candidate.destroy();
          continue;
        }
        slot = candidate;
        reused = true;
        break;
      }
      if (free.length === 0) delete agent.freeSockets[name];
      const totalSockets = Object.values(agent.sockets).reduce((count, pool) => count + pool.length, 0) +
        Object.values(agent.freeSockets).reduce((count, pool) => count + pool.length, 0);
      const queued = !slot &&
        (sockets.length >= agent.maxSockets || totalSockets >= agent.maxTotalSockets);
      if (queued) slot = sockets[0] || {};
      else if (!slot) slot = {};
      if (!queued) {
        if (reused) delete slot.__quenchHttpFreeSocketGuard;
        sockets.push(slot);
        if (free.length === 0) delete agent.freeSockets[name];
      } else {
        (agent.requests[name] || (agent.requests[name] = [])).push(slot);
      }
      agentPool = { name, slot, queued, reused, lastQueued: !queued };
    }
    let customSocket;
    const customWrites = [];
    let customResponseBuffer = Buffer.alloc(0);
    let customResponseStarted = false;
    let pendingExplicitWrites = 0;
    let pendingSocketBodyBytes = 0;
    const request = new Writable({
      // Request-body completion precedes response completion. Auto-destroy
      // would call `_destroy` at `finish` and close the live HTTP exchange.
      highWaterMark: options?.highWaterMark ?? DEFAULT_HTTP_HIGH_WATER_MARK,
      autoDestroy: false,
      write(chunk, encoding, callback) {
        if (request.__quenchRequestStarted) {
          const length = Buffer.isBuffer(chunk)
            ? chunk.length
            : Buffer.byteLength(String(chunk), typeof encoding === 'string' ? encoding : 'utf8');
          if (length === 0) { process.nextTick(callback); return; }
          const error = new Error('Request body exceeds Content-Length');
          error.code = 'ERR_HTTP_CONTENT_LENGTH_MISMATCH';
          process.nextTick(callback, error);
          return;
        }
        // `end(chunk)` can flush after previously queued writes, even though
        // the writable is already marked as ending. Count calls through the
        // public write() method so buffered writes stay distinct from that
        // final chunk.
        const explicit = pendingExplicitWrites > 0;
        if (explicit) pendingExplicitWrites--;
        const length = Buffer.isBuffer(chunk)
          ? chunk.length
          : Buffer.byteLength(String(chunk), typeof encoding === 'string' ? encoding : 'utf8');
        pendingSocketBodyBytes += length;
        if (request.socket && pendingSocketBodyBytes > 0) {
          request.socket.__quenchHttpRequestWritePending = pendingSocketBodyBytes;
        }
        try {
          if (customSocket) customWrites.push(Buffer.from(chunk));
          else write(id, chunk, explicit);
          process.nextTick(callback);
        }
        catch (error) { process.nextTick(callback, error); }
      },
      final(callback) {
        if (request.__quenchRequestStarted) { callback(); return; }
        if (customSocket) {
          try {
            // A caller-supplied Duplex owns this request's wire. Avoid opening
            // the host TCP connection stored in the ordinary HTTP request.
            destroy(id);
            startCustomSocketResponse();
            customSocket.write(request._header || '');
            for (const chunk of customWrites) customSocket.write(chunk);
            callback();
          } catch (error) { callback(error); }
          return;
        }
        if (agent && typeof agent.createSocket === 'function' && !request.socket) {
          request.once('socket', () => {
            try { end(id, request); callback(); }
            catch (error) { callback(error); }
          });
          return;
        }
        if (customSocket && customSocket.connecting) {
          const onError = (error) => {
            customSocket.removeListener('connect', onConnect);
            callback(error);
          };
          const onConnect = () => {
            customSocket.removeListener('error', onError);
            try { end(id, request); callback(); }
            catch (error) { callback(error); }
          };
          customSocket.once('connect', onConnect);
          customSocket.once('error', onError);
          return;
        }
        try { end(id, request); callback(); }
        catch (error) { callback(error); }
      },
      destroy(error, callback) {
        try {
          let transportError = error;
          if (!transportError && request.aborted && request.res) {
            transportError = new Error('aborted');
            transportError.code = 'ECONNRESET';
          }
          destroy(id, transportError);
          callback(error);
        }
        catch (failure) { callback(failure); }
      },
    });
    Object.setPrototypeOf(request, clientRequestPrototype);
    // Node's OutgoingMessage keeps this legacy public flag true after end();
    // the underlying Writable state still rejects subsequent writes.
    Object.defineProperty(request, 'writable', {
      configurable: true,
      enumerable: true,
      writable: true,
      value: true,
    });
    request.__quenchTrailers = [];
    request.maxHeadersCount = null;
    // ClientRequest.destroy() exposes the destroyed state synchronously. The
    // Writable shim can defer its internal destroy bookkeeping, so keep the
    // public flag on the request itself and update it before delegating.
    const destroyRequest = request.destroy.bind(request);
    Object.defineProperty(request, 'destroyed', {
      configurable: true,
      enumerable: true,
      writable: true,
      value: false,
    });
    const releaseAgentSlotAfterDestroy = () => {
      if (!agentPool || agentPool.queued || request.__quenchAgentSlotReleased) return;
      request.__quenchAgentSlotReleased = true;
      const { name, slot } = agentPool;
      const sockets = agent.sockets[name];
      const index = sockets?.indexOf(slot) ?? -1;
      if (index !== -1) sockets.splice(index, 1);
      if (sockets && sockets.length === 0) delete agent.sockets[name];
      let requests = agent.requests[name];
      let waitingRequest;
      while (requests?.length) {
        waitingRequest = requests.shift();
        if (!waitingRequest?.destroyed) break;
        waitingRequest = undefined;
      }
      if (requests && requests.length === 0) {
        delete agent.requests[name];
        requests = undefined;
      }
      if (!waitingRequest) return;
      const waitingPool = waitingRequest.__quenchAgentPool;
      if (waitingPool) {
        waitingPool.slot = {};
        waitingPool.reused = false;
        waitingPool.queued = false;
        if (!agent.sockets[name]) agent.sockets[name] = [];
        agent.sockets[name].push(waitingPool.slot);
      }
      waitingRequest.reusedSocket = false;
      waitingRequest.__quenchCreateSocket?.();
    };
    request.destroy = (error) => {
      request.destroyed = true;
      // Explicit errors (socket failures and parser failures) are terminal;
      // they must not wait for an HTTP response end that can never arrive.
      if (error) request.__quenchCloseReady = true;
      // An in-flight request destroyed by the caller has no response end to
      // release the Agent's deferred close. Node emits close after tearing
      // down its socket, so let the Writable close notification through.
      if (!request.res) request.__quenchCloseReady = true;
      // A request destroyed before receiving a response has no response-end
      // callback to release its Agent slot. Remove it now and give the next
      // live queued request a fresh connection; queued requests remain in the
      // public queue until the current slot is actually released.
      if (agentPool && !agentPool.queued) releaseAgentSlotAfterDestroy();
      // Destroying a ClientRequest before its Agent has assigned a socket is
      // still an HTTP request failure in Node. The Writable has no transport
      // to report the usual ECONNRESET from, so synthesize that error after
      // the caller's synchronous listener-registration turn.
      if (!error && !request.res && !request.aborted &&
          !request.__quenchDestroyErrorScheduled) {
        request.__quenchDestroyErrorScheduled = true;
        setTimeout(() => {
          if (request.__quenchDestroyErrorEmitted) return;
          request.__quenchDestroyErrorEmitted = true;
          const failure = new Error('socket hang up');
          failure.code = 'ECONNRESET';
          request.__quenchCloseReady = true;
          // Let Writable's normal destroy lifecycle deliver error then close.
          // Emitting an error and HTTP close manually here races the close
          // notification produced by the underlying Writable and can expose
          // two ClientRequest close events.
          destroyRequest(failure);
        }, 0);
        return request;
      }
      return destroyRequest(error);
    };
    const startCustomSocketResponse = () => {
      if (!customSocket || customResponseStarted) return;
      customResponseStarted = true;
      const onData = (chunk) => {
        customResponseBuffer = Buffer.concat([customResponseBuffer, Buffer.from(chunk)]);
        while (true) {
          const headerEnd = customResponseBuffer.indexOf('\r\n\r\n');
          if (headerEnd === -1) return;
          if (typeof maxHeaderSize === 'number' && maxHeaderSize > 0 &&
              headerEnd + 4 > maxHeaderSize) {
            const error = new Error('Parse Error');
            error.code = 'HPE_HEADER_OVERFLOW';
            request.destroy(error);
            return;
          }
          const head = customResponseBuffer.subarray(0, headerEnd).toString('latin1');
          const lines = head.split('\r\n');
          const status = /^HTTP\/(\d+)\.(\d+)\s+(\d+)(?:\s+(.*))?$/.exec(lines.shift() || '');
          if (!status) {
            const error = new Error('Parse Error');
            error.code = 'HPE_INVALID_STATUS';
            request.destroy(error);
            return;
          }
          const headers = Object.create(null);
          const rawHeaders = [];
          for (const line of lines) {
            const colon = line.indexOf(':');
            if (colon <= 0) continue;
            const rawName = line.slice(0, colon).trim();
            const value = line.slice(colon + 1).trim();
            const name = rawName.toLowerCase();
            const invalid = relaxedHeaderValues
              ? /[\x00\x0a\x0d]|[^\x00-\xff]/
              : /[^\t\x20-\x7e\x80-\xff]/;
            if (invalid.test(value)) {
              const error = new Error('Parse Error');
              error.code = 'HPE_INVALID_HEADER_TOKEN';
              request.destroy(error);
              return;
            }
            rawHeaders.push(rawName, value);
            headers[name] = headers[name] === undefined ? value : `${headers[name]}, ${value}`;
          }
          const maxHeadersCount = request.maxHeadersCount;
          if (typeof maxHeadersCount === 'number' && maxHeadersCount > 0 &&
              rawHeaders.length > maxHeadersCount * 2) {
            rawHeaders.length = maxHeadersCount * 2;
            for (const name of Object.keys(headers)) delete headers[name];
            for (let index = 0; index < rawHeaders.length; index += 2) {
              const name = rawHeaders[index].toLowerCase();
              const value = rawHeaders[index + 1];
              headers[name] = headers[name] === undefined ? value : `${headers[name]}, ${value}`;
            }
          }
          const statusCode = Number(status[3]);
          customResponseBuffer = customResponseBuffer.subarray(headerEnd + 4);
          if (statusCode >= 100 && statusCode < 200 && statusCode !== 101) {
            request.emit('information', {
              statusCode,
              statusMessage: status[4] || '',
              httpVersion: `${status[1]}.${status[2]}`,
              httpVersionMajor: Number(status[1]),
              httpVersionMinor: Number(status[2]),
              headers,
              rawHeaders,
            });
            continue;
          }
          customSocket.removeListener('data', onData);
          const response = new Readable({ read() {} });
          response.statusCode = statusCode;
          response.statusMessage = status[4] || '';
          response.httpVersion = `${status[1]}.${status[2]}`;
          response.httpVersionMajor = Number(status[1]);
          response.httpVersionMinor = Number(status[2]);
          response.headers = headers;
          response.rawHeaders = rawHeaders;
          // The native parser normally reports this flag on its response
          // callback. Keep the public ClientRequest field in sync with the
          // response protocol and Connection tokens so callers and Agent
          // reuse logic see Node's HTTP/1.0 vs HTTP/1.1 behavior.
          const connectionTokens = String(headers.connection || '')
            .toLowerCase()
            .split(',')
            .map((token) => token.trim());
          request.shouldKeepAlive = connectionTokens.includes('keep-alive') ||
            ((response.httpVersionMajor > 1 ||
              (response.httpVersionMajor === 1 && response.httpVersionMinor >= 1)) &&
              !connectionTokens.includes('close'));
          request.emit('response', response);
          if (customResponseBuffer.length) response.push(customResponseBuffer);
          response.push(null);
          return;
        }
      };
      customSocket.on('data', onData);
      customSocket.on('error', (error) => request.destroy(error));
    };
    const writeChunk = request.write.bind(request);
    let requestBodyBytes = 0;
    function startLengthDelimitedRequest() {
      if (request.__quenchRequestStarted || request.__quenchEndRequested || customSocket ||
          expectedContentLength === undefined || !Number.isSafeInteger(expectedContentLength) ||
          expectedContentLength < 0 || requestBodyBytes !== expectedContentLength || !request.socket) {
        return;
      }
      request.__quenchRequestStarted = true;
      try { end(id, request); }
      catch (error) { request.destroy(error); }
    }
    request.write = (chunk, encoding, callback) => {
      pendingExplicitWrites++;
      try {
        const result = writeChunk(chunk, encoding, callback);
        requestBodyBytes += Buffer.isBuffer(chunk)
          ? chunk.length
          : Buffer.byteLength(String(chunk), typeof encoding === 'string' ? encoding : 'utf8');
        startLengthDelimitedRequest();
        return result;
      }
      catch (error) { pendingExplicitWrites--; throw error; }
    };
    request.setHeader = (name, value) => {
      validateHeaderValue(name, value);
      const pair = headerPairs([name, value], uniqueHeaderNames);
      replaceHeaderPairs(requestHeaders, name, pair);
      setHeader(id, pair, relaxedHeaderValues);
      return request;
    };
    request.getHeader = (name) => {
      const key = String(name).toLowerCase();
      const values = [];
      for (let index = 0; index < requestHeaders.length; index += 2) {
        if (String(requestHeaders[index]).toLowerCase() === key) {
          values.push(requestHeaders[index + 1]);
        }
      }
      return values.length > 1 ? values : values[0];
    };
    request.getHeaders = () => {
      const values = Object.create(null);
      for (let index = 0; index < requestHeaders.length; index += 2) {
        const name = String(requestHeaders[index]).toLowerCase();
        if (Object.hasOwn(values, name)) {
          if (!Array.isArray(values[name])) values[name] = [values[name]];
          values[name].push(requestHeaders[index + 1]);
        } else {
          values[name] = requestHeaders[index + 1];
        }
      }
      return values;
    };
    request.getHeaderNames = () => {
      const names = [];
      for (let index = 0; index < requestHeaders.length; index += 2) {
        const name = String(requestHeaders[index]).toLowerCase();
        if (!names.includes(name)) names.push(name);
      }
      return names;
    };
    request.hasHeader = (name) => request.getHeader(name) !== undefined;
    request.addTrailers = (trailers) => {
      if (trailers === null || typeof trailers !== 'object') {
        const error = new TypeError('The "headers" argument must be an object');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      const pairs = headerPairs(trailers, uniqueHeaderNames);
      for (let index = 0; index < pairs.length; index += 2) {
        validateHeaderValue(pairs[index], pairs[index + 1]);
      }
      request.__quenchTrailers = pairs;
      return request;
    };
    request.appendHeader = (name, value) => {
      validateHeaderValue(name, value);
      const key = String(name).toLowerCase();
      const previous = request.getHeader(key);
      let originalName = name;
      for (let index = 0; index < requestHeaders.length; index += 2) {
        if (String(requestHeaders[index]).toLowerCase() === key) {
          originalName = requestHeaders[index];
          break;
        }
      }
      const added = Array.isArray(value) ? value : [value];
      const combined = previous === undefined ? added
        : [...(Array.isArray(previous) ? previous : [previous]), ...added];
      // Use setHeader so the host-side header table and the request's public
      // header snapshot remain in sync. Node joins repeated values for most
      // fields, while cookie values use the cookie-specific separator.
      request.setHeader(originalName, combined);
      return request;
    };
    request.removeHeader = (name) => {
      const key = String(name).toLowerCase();
      for (let index = requestHeaders.length - 2; index >= 0; index -= 2) {
        if (String(requestHeaders[index]).toLowerCase() === key) requestHeaders.splice(index, 2);
      }
      removeHeader(id, name);
      return request;
    };
    // Publish `created` only after the option headers and their query methods
    // are visible on the request. `start` belongs to socket assignment (or a
    // synchronous custom-socket assignment), after callers had their
    // construction turn to set additional headers.
    const diagnosticsMessage = { request };
    let diagnosticsStarted = false;
    const publishStart = () => {
      if (diagnosticsStarted) return;
      diagnosticsStarted = true;
      publishDiagnostics('http.client.request.start', diagnosticsMessage);
    };
    publishDiagnostics('http.client.request.created', diagnosticsMessage);
    request.once('error', (error) => {
      publishDiagnostics('http.client.request.error', { request, error });
    });
    request.once('response', (response) => {
      response.once('end', () => {
        if (request.timeoutCb && request.socket) {
          request.socket.removeListener('timeout', request.timeoutCb);
          request.timeoutCb = null;
        }
        publishDiagnostics('http.client.response.finish', { request, response });
      });
    });
    request.once('close', () => {
      if (request.timeoutCb && request.socket) {
        request.socket.removeListener('timeout', request.timeoutCb);
        request.timeoutCb = null;
      }
    });
    const finish = request.end.bind(request);
    request.end = (chunk, encoding, callback) => {
      request.__quenchEndRequested = true;
      request._header = `${request.method} ${request.path} HTTP/1.1\r\n${requestHeaders.map((value, index) => index % 2 === 0 ? `${value}: ` : `${value}\r\n`).join('')}\r\n`;
      const result = finish(chunk, encoding, callback);
      if (timeout > 0) request.setTimeout(timeout);
      return result;
    };
    let currentPath;
    Object.defineProperty(request, 'path', {
      configurable: true,
      enumerable: true,
      get() { return currentPath; },
      set(value) { currentPath = validatePath(value); },
    });
    request.path = requestPath;
    request.method = options?.method || "GET";
    request.host = optionHostname || options?.host || "localhost";
    request.protocol = options?.protocol || "http:";
    request.port = options?.port || (request.protocol === 'https:' ? 443 : 80);
    request.timeout = timeout;
    request.localAddress = options?.localAddress;
    request.reusedSocket = !!agentPool?.reused;
    request.onSocket = (socket, error) => {
      if (error) {
        // Node routes Agent connection failures through onSocket(socket, err)
        // so ClientRequest emits its error and close events without asking
        // Writable.destroy() to reinterpret a socket-creation failure as a
        // request-body failure.
        destroy(id);
        request.destroyed = true;
        // Quench's setImmediate shim is a Promise microtask, so a nested
        // nextTick can reach it before `http.get()` returns and the caller
        // attaches its error listener. Use a timer turn for this callback;
        // Node's onSocketNT runs after the synchronous call stack has unwound.
        setTimeout(() => {
          request.emit('error', error);
          request.__quenchCloseReady = true;
          request.__quenchEmitClose();
        }, 0);
        return;
      }
      // Agent.createConnection is overridable and may return an arbitrary
      // Duplex (for example a JSStreamSocket), not a Quench TCP Socket with a
      // native transport id. Route such sockets through the supplied-Duplex
      // HTTP path so the request does not open an unrelated localhost:80
      // connection in the native client.
      if (socket && !socket.__quenchNetSocketId &&
          typeof socket.write === 'function' && typeof socket.on === 'function') {
        customSocket = socket;
      }
      if (agentPool && socket) {
        const previousSlot = agentPool.slot;
        const sockets = agent.sockets[agentPool.name];
        const index = sockets?.indexOf(previousSlot) ?? -1;
        if (index !== -1) sockets[index] = socket;
        agentPool.slot = socket;
      }
      if (socket && typeof socket.removeListener === 'function' &&
          socket.__quenchFreeSocketErrorListener) {
        socket.removeListener('error', socket.__quenchFreeSocketErrorListener);
      }
      request.socket = socket;
      if (pendingSocketBodyBytes > 0) {
        socket.__quenchHttpRequestWritePending = pendingSocketBodyBytes;
      }
      publishStart();
      startLengthDelimitedRequest();
      // A failed DNS lookup or TCP connect is surfaced by the assigned
      // net.Socket. Node's ClientRequest forwards that failure to its own
      // error channel; without this bridge, requests to unresolved hosts stay
      // pending forever after the socket emits its error.
      if (socket && typeof socket.once === 'function') {
        const onSocketError = (error) => request.destroy(error);
        socket.once('error', onSocketError);
        request.once('response', () => socket.removeListener('error', onSocketError));
      }
      socket._httpMessage = request;
      request.emit('socket', socket);
    };
    if (typeof options?.createConnection === 'function') {
      try {
        customSocket = options.createConnection(options);
        request.onSocket(customSocket);
      } catch (error) {
        process.nextTick(() => request.destroy(error));
      }
    }
    request.aborted = false;
    request.abort = () => {
      if (request.aborted) return request;
      request.aborted = true;
      request.destroy();
      queueMicrotask(() => request.emit('abort'));
      return request;
    };
    const signal = options?.signal;
    if (signal && typeof signal.addEventListener === 'function') {
      const abortWithSignal = () => {
        if (request.destroyed) return;
        const error = new Error('The operation was aborted');
        error.name = 'AbortError';
        error.code = 'ABORT_ERR';
        request.destroy(error);
      };
      const removeAbortListener = () => signal.removeEventListener('abort', abortWithSignal);
      if (signal.aborted) queueMicrotask(abortWithSignal);
      else {
        signal.addEventListener('abort', abortWithSignal, { once: true });
        request.once('close', removeAbortListener);
      }
    }
    let pendingTimeoutSocket;
    let pendingTimeoutApply;
    const emitRequestTimeout = function emitRequestTimeout() {
      const request = this._httpMessage;
      if (request) request.emit('timeout');
    };
    const listenSocketTimeout = () => {
      if (request.timeoutCb) return;
      request.timeoutCb = emitRequestTimeout;
      const attach = (socket) => {
        if (!socket || typeof socket.once !== 'function') return;
        // The Node ClientRequest source may already have installed its
        // timeout forwarder before this adapter's setTimeout path runs.
        if (typeof socket.listenerCount === 'function' && socket.listenerCount('timeout') > 0) return;
        socket.once('timeout', emitRequestTimeout);
      };
      if (request.socket) {
        const socket = request.socket;
        if (socket.connecting) queueMicrotask(() => attach(socket));
        else attach(socket);
      }
      else request.once('socket', (socket) => {
        // Node's `_http_client` also installs its `onTimeout` forwarder while
        // handling this same socket event. Defer the fallback check until
        // that synchronous listener pass has completed to avoid a duplicate.
        queueMicrotask(() => attach(socket));
      });
    };
    request.setTimeout = (milliseconds, callback) => {
      listenSocketTimeout();
      if (typeof callback === 'function') request.once('timeout', callback);
      // ClientRequest timeout is a socket inactivity timeout. If set before
      // the connection completes, Node preserves the Agent's timeout until
      // `connect`, then applies the request's override. Keep that ordering so
      // the public `socket.timeout` value is observable in the same way.
      const applyTimeout = (socket) => {
        if (!socket || typeof socket.setTimeout !== 'function') return;
        if (pendingTimeoutSocket && pendingTimeoutApply) {
          pendingTimeoutSocket.removeListener('connect', pendingTimeoutApply);
          pendingTimeoutSocket = undefined;
          pendingTimeoutApply = undefined;
        }
        if (socket.connecting) {
          pendingTimeoutSocket = socket;
          pendingTimeoutApply = () => {
            pendingTimeoutSocket = undefined;
            pendingTimeoutApply = undefined;
            socket.setTimeout(milliseconds);
          };
          socket.once('connect', pendingTimeoutApply);
        } else {
          socket.setTimeout(milliseconds);
        }
      };
      if (request.socket) applyTimeout(request.socket);
      else request.once('socket', applyTimeout);
      return request;
    };
    request.clearTimeout = (callback) => {
      request.setTimeout(0);
      if (typeof callback === 'function') request.once('timeout', callback);
      return request;
    };
    request.setNoDelay = () => request;
    request.setSocketKeepAlive = () => request;
    request.flushHeaders = () => request;
    request.once('finish', () => {
      if (request.socket) delete request.socket.__quenchHttpRequestWritePending;
    });
    const emitRequest = request.emit.bind(request);
    if (agentPool) {
      // Writable's own close can precede the HTTP response end. Keep it
      // private until the Agent has released or handed off the socket.
      request.emit = (event, ...args) => {
        if (event === 'close' && !request.__quenchCloseReady) {
          request.__quenchClosePending = true;
          return true;
        }
        return emitRequest(event, ...args);
      };
    }
    request.__quenchEmitClose = () => {
      if (request._httpCloseEmitted) return;
      // The native parser calls this as soon as it pushes response EOF, before
      // Readable emits its `end` event. Hold close until the response/Agent
      // lifecycle observes end and releases the socket.
      if (!request.__quenchCloseReady) {
        request.__quenchClosePending = true;
        return;
      }
      request._httpCloseEmitted = true;
      emitRequest('close');
    };
    if (agentPool?.queued) {
      const queue = agent.requests[agentPool.name];
      const placeholder = queue?.indexOf(agentPool.slot) ?? -1;
      if (placeholder !== -1) queue[placeholder] = request;
      request.__quenchAgentPool = agentPool;
    }
    if (agentPool) {
      request.once('response', (response) => {
        const queued = agent.requests[agentPool.name];
        agentPool.lastQueued = !queued || queued.length === 0;
        let settled = false;
        let responseEnded = false;
        const settleAgentRequest = () => {
          if (settled || !responseEnded || !request.writableFinished) return;
          settled = true;
          request.destroyed = true;
          const { name, slot } = agentPool;
          let requests = agent.requests[name];
          let waitingRequest;
          // Aborted requests can remain in Agent.requests until the active
          // socket is released. Retire those queue entries here so they never
          // acquire a socket or reach the server.
          while (requests?.length) {
            waitingRequest = requests.shift();
            if (!waitingRequest?.destroyed) break;
            waitingRequest = undefined;
          }
          if (requests && requests.length === 0) {
            delete agent.requests[name];
            requests = undefined;
          }
          process.nextTick(() => {
            const sockets = agent.sockets[name];
            if (sockets) {
              const index = sockets.indexOf(slot);
              if (index !== -1) sockets.splice(index, 1);
            }
            if (waitingRequest && typeof waitingRequest.onSocket === 'function') {
              const waitingPool = waitingRequest.__quenchAgentPool;
              // A queued request gets the just-released socket directly, so
              // it must perform the same synchronous pending-read check as a
              // request taking a socket from freeSockets. If data is already
              // waiting in the kernel, discard this connection and give the
              // queued request a fresh one instead of parsing unsolicited
              // bytes as its response.
              if (!agent.keepAlive || !request.socket || request.socket.destroyed ||
                  request.socket._handle?.hasPendingRead?.() ||
                  request.socket._handle?.writeQueueSize > 0) {
                request.socket?.destroy();
                if (waitingPool) {
                  waitingPool.slot = {};
                  waitingPool.reused = false;
                  waitingPool.queued = false;
                  if (!agent.sockets[name]) agent.sockets[name] = [];
                  agent.sockets[name].push(waitingPool.slot);
                }
                waitingRequest.reusedSocket = false;
                waitingRequest.__quenchCreateSocket?.();
                request.__quenchCloseReady = true;
                request.__quenchEmitClose();
                return;
              }
              if (waitingPool) {
                waitingPool.slot = request.socket;
                waitingPool.reused = true;
                waitingPool.queued = false;
              }
              waitingRequest.reusedSocket = true;
              if (!agent.sockets[name]) agent.sockets[name] = [];
              agent.sockets[name].push(request.socket);
              waitingRequest.onSocket(request.socket);
              request.__quenchCloseReady = true;
              request.__quenchEmitClose();
              return;
            }
            if (requests) {
              request.__quenchCloseReady = true;
              request.__quenchEmitClose();
              return;
            }
            if (agent.keepAlive && request.socket && !request.socket.destroyed) {
              const socket = request.socket;
              const timeout = request.timeout || agent.options.timeout || 0;
              if (timeout) socket.setTimeout(timeout);
              if (typeof agent.keepSocketAlive !== 'function' || agent.keepSocketAlive(socket)) {
                const free = agent.freeSockets[name] || (agent.freeSockets[name] = []);
                socket._httpMessage = null;
                // Node detaches its HTTP parser before exposing an idle
                // keep-alive socket through Agent.freeSockets.
                socket.parser = null;
                socket[asyncIdSymbol] = -1;
                // Observe unsolicited input through net's internal delivery
                // path, without adding public `data`/`readable` listeners.
                socket.__quenchHttpFreeSocketGuard = () => {
                  delete socket.__quenchHttpFreeSocketGuard;
                  const idle = agent.freeSockets[name];
                  const index = idle?.indexOf(socket) ?? -1;
                  if (index !== -1) {
                    idle.splice(index, 1);
                    if (idle.length === 0) delete agent.freeSockets[name];
                  }
                  socket.destroy();
                };
                free.push(socket);
                if (socket.__quenchFreeSocketErrorListener) {
                  socket.once('error', socket.__quenchFreeSocketErrorListener);
                }
                socket.emit('free');
              } else {
                socket.destroy();
              }
            }
            if (agent.sockets[name] && agent.sockets[name].length === 0) delete agent.sockets[name];
            // Node makes the released socket visible to close handlers.
            request.__quenchCloseReady = true;
            request.__quenchEmitClose();
          });
        };
        response.once('end', () => {
          responseEnded = true;
          settleAgentRequest();
        });
        request.once('finish', settleAgentRequest);
      });
    }
    // Agent requests emit close from the Agent's release tick above. Requests
    // without an Agent still close after the response has ended.
    if (!agentPool) {
      request.once('response', (response) => {
        let responseEnded = false;
        const emitResponseClose = () => {
          if (!responseEnded || !request.writableFinished) return;
          request.destroyed = true;
          request.__quenchCloseReady = true;
          process.nextTick(request.__quenchEmitClose);
        };
        response.once('end', () => { responseEnded = true; emitResponseClose(); });
        request.once('finish', emitResponseClose);
      });
    }
    request.__quenchCreateSocket = () => {
      if (request.destroyed) return;
      agent.createSocket(request, options || {}, (error, socket) => {
        if (error) {
          request.onSocket(socket, error);
          return;
        }
        if (socket) request.onSocket(socket);
      });
    };
    // Node asks the agent for a socket while constructing ClientRequest, even
    // when the caller never calls end(). Keep callback errors observable on
    // the returned request and defer invocation so callers can attach listeners.
    if (!customSocket && agent && typeof agent.createSocket === "function") {
      process.nextTick(() => {
        // A request can be destroyed synchronously before this deferred
        // socket allocation runs (for example, header-validation probes).
        // Do not open a connection for an already-cancelled request.
        if (request.destroyed || agentPool?.queued) return;
        try {
          if (agentPool?.reused && agentPool.slot) {
            // Give a pooled TCP handle a new async identity in this request's
            // context before delivering its socket callbacks.
            asyncResetHandle(agentPool.slot);
            // Node synchronizes the writable high-water mark when an Agent
            // reassigns a pooled socket to a request with a different value.
            // The stream state is mutable so backpressure thresholds follow
            // the new request without replacing the socket.
            if (options?.highWaterMark != null &&
                agentPool.slot.writableHighWaterMark !== options.highWaterMark &&
                agentPool.slot._writableState) {
              agentPool.slot._writableState.highWaterMark = options.highWaterMark;
            }
            if (typeof agent.trackSocketEnd === 'function') {
              const name = agentPool.name;
              const socket = agentPool.slot;
              const removeFromPools = () => {
                for (const pool of [agent.sockets[name], agent.freeSockets[name]]) {
                  if (!pool) continue;
                  const index = pool.indexOf(socket);
                  if (index !== -1) pool.splice(index, 1);
                }
                if (agent.sockets[name]?.length === 0) delete agent.sockets[name];
                if (agent.freeSockets[name]?.length === 0) delete agent.freeSockets[name];
              };
              agent.trackSocketEnd(socket, name, removeFromPools);
            }
            request.onSocket(agentPool.slot);
          } else {
            request.__quenchCreateSocket();
          }
        } catch (error) {
          request.destroy(error);
        }
      });
    }
    if (typeof callback === "function") request.once("response", callback);
    return request;
  }
  request.prototype = clientRequestPrototype;
  Object.defineProperty(clientRequestPrototype, 'constructor', {
    configurable: true, writable: true, value: request,
  });
  return request;
})"#;

pub(crate) fn request_function(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let create = context.host_function(crate::host::shared_vm::operation("httpRequestCreate"))?;
    let write = context.host_function(crate::host::shared_vm::operation("httpRequestWrite"))?;
    let set_header =
        context.host_function(crate::host::shared_vm::operation("httpRequestSetHeader"))?;
    let remove_header =
        context.host_function(crate::host::shared_vm::operation("httpRequestRemoveHeader"))?;
    let end = context.host_function(crate::host::shared_vm::operation("httpRequestEnd"))?;
    let destroy = context.host_function(crate::host::shared_vm::operation("httpRequestDestroy"))?;
    let normalize_url =
        context.host_function(crate::host::shared_vm::operation("httpRequestNormalizeUrl"))?;
    let publish_diagnostics =
        context.host_function(crate::host::shared_vm::operation("diagnosticsPublish"))?;
    let symbols = super::super::util_shared_vm::symbols_binding(context)?;
    let async_id_key = context.string_rooted("async_id_symbol");
    let async_id_symbol = context.get_property_rooted(symbols, async_id_key)?;
    let stream = crate::host::shared_vm::commonjs::stream_module(context)?;
    let writable_key = context.string_rooted("Writable");
    let writable = context.get_property_rooted(stream, writable_key)?;
    let readable_key = context.string_rooted("Readable");
    let readable = context.get_property_rooted(stream, readable_key)?;
    let factory = context.evaluate_script_rooted(REQUEST_FUNCTION, "node:http/request.js")?;
    let undefined = context.undefined();
    context.call_rooted(
        factory,
        undefined,
        &[
            writable,
            readable,
            create,
            write,
            set_header,
            remove_header,
            end,
            destroy,
            normalize_url,
            publish_diagnostics,
            async_id_symbol,
        ],
    )
}

pub(crate) fn normalize_url(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = match args.first().copied() {
        Some(root) => context.string_text(root)?,
        None => None,
    };
    let Some(input) = input else {
        return invalid_url(context, "The \"url\" argument must be of type string");
    };
    let parsed = match url::Url::parse(&input) {
        Ok(parsed) => parsed,
        Err(_) => return invalid_url(context, "Invalid URL"),
    };
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        let error = context.type_error_rooted(&format!(
            "Protocol \"{}:\" not supported. Expected \"http:\" or \"https:\"",
            parsed.scheme()
        ))?;
        set_text(context, error, "code", "ERR_INVALID_PROTOCOL")?;
        return Err(context.throw(error));
    }
    let Some(hostname) = parsed.host_str() else {
        return invalid_url(context, "Invalid URL");
    };
    let port = parsed.port_or_known_default().unwrap_or(80);
    let mut path = parsed.path().to_owned();
    if let Some(query) = parsed.query() {
        path.push('?');
        path.push_str(query);
    }
    let options = context.object_rooted()?;
    set_text(context, options, "hostname", hostname)?;
    let port = context.number(f64::from(port));
    set(context, options, "port", port)?;
    set_text(
        context,
        options,
        "protocol",
        &format!("{}:", parsed.scheme()),
    )?;
    set_text(context, options, "path", &path)?;
    set_text(context, options, "method", "GET")?;
    Ok(options)
}

fn invalid_url(context: &mut Context<'_>, message: &str) -> Result<RootId, RootedError> {
    let error = context.type_error_rooted(message)?;
    set_text(context, error, "code", "ERR_INVALID_URL")?;
    Err(context.throw(error))
}

fn set_text(
    context: &mut Context<'_>,
    object: RootId,
    name: &str,
    value: &str,
) -> Result<(), RootedError> {
    let value = context.string_rooted(value);
    set(context, object, name, value)
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
            "cannot initialize shared HTTP request option {name}"
        )))
    }
}

pub(crate) fn create(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let options = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("http.request options are missing"))?;
    let host = text_property(context, options, "hostname")?
        .or(text_property(context, options, "host")?)
        .unwrap_or_else(|| "localhost".to_owned());
    let port = numeric_property(context, options, "port")?
        .filter(|port| port.is_finite() && (0.0..=u16::MAX as f64).contains(port))
        .unwrap_or(80.0) as u16;
    let method = text_property(context, options, "method")?.unwrap_or_else(|| "GET".to_owned());
    let path = text_property(context, options, "path")?.unwrap_or_else(|| "/".to_owned());
    let validation = text_property(context, options, "httpValidation")?;
    let relaxed = matches!(validation.as_deref(), Some("relaxed" | "insecure"));
    let headers = read_headers(context, args.get(1).copied(), relaxed)?;
    let set_default_headers = args
        .get(2)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let omit_host = args
        .get(3)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let agent = object_property(context, options, "agent")?
        .map(|agent| property_id(context, agent, "_quenchSharedAgentId"))
        .transpose()?
        .flatten();
    let state = context.host_mut().shared_state();
    let id = state
        .borrow_mut()
        .http
        .request_id()
        .map_err(RootedError::host)?;
    // Bind the response callbacks to the async context that created this
    // ClientRequest so AsyncLocalStorage survives the transport boundary.
    let async_id = super::super::async_hooks_shared_vm::create_context(&state);
    let mut host_state = state.borrow_mut();
    host_state.http.requests.insert(
        id,
        super::state::OutgoingRequest {
            async_id,
            host,
            port,
            method,
            path,
            headers,
            trailers: Vec::new(),
            set_default_headers,
            omit_host,
            body: Default::default(),
            agent,
        },
    );
    Ok(context.number(id as f64))
}

pub(crate) fn write(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let chunk = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("HTTP request chunk is missing"))?;
    let chunk = context.to_string(chunk)?.into_bytes();
    let explicit = args
        .get(2)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let state = context.host_mut().shared_state();
    let mut host = state.borrow_mut();
    let Some(request) = host.http.requests.get_mut(&id) else {
        return Err(RootedError::host(
            "shared HTTP request is no longer writable",
        ));
    };
    request.body.push(chunk, explicit);
    Ok(context.boolean(true))
}

pub(crate) fn remove_header(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let name = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("HTTP request header name is missing"))?;
    let name = context.to_string(name)?;
    if let Some(request) = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
        .requests
        .get_mut(&id)
    {
        request
            .headers
            .retain(|(key, _)| !key.eq_ignore_ascii_case(&name));
    }
    Ok(context.undefined())
}

pub(crate) fn set_header(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let relaxed = args
        .get(2)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let headers = read_headers(context, args.get(1).copied(), relaxed)?;
    let state = context.host_mut().shared_state();
    if let Some(request) = state.borrow_mut().http.requests.get_mut(&id) {
        let header_count = headers.len();
        let mut replacements: Vec<(String, Vec<(String, String)>)> = Vec::new();
        for (name, value) in headers {
            if let Some((_, values)) = replacements
                .iter_mut()
                .find(|(key, _)| key.eq_ignore_ascii_case(&name))
            {
                values.push((name, value));
            } else {
                replacements.push((name.clone(), vec![(name, value)]));
            }
        }
        let mut inserted = vec![false; replacements.len()];
        let mut updated = Vec::with_capacity(request.headers.len() + header_count);
        for (name, value) in std::mem::take(&mut request.headers) {
            if let Some(index) = replacements
                .iter()
                .position(|(key, _)| key.eq_ignore_ascii_case(&name))
            {
                if !inserted[index] {
                    updated.append(&mut replacements[index].1);
                    inserted[index] = true;
                }
            } else {
                updated.push((name, value));
            }
        }
        for (index, (_, values)) in replacements.iter_mut().enumerate() {
            if !inserted[index] {
                updated.append(values);
            }
        }
        request.headers = updated;
    }
    Ok(context.undefined())
}

pub(crate) fn end(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let request_root = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("HTTP ClientRequest is missing"))?;
    let mut request = {
        let state = context.host_mut().shared_state();
        let mut guard = state.borrow_mut();
        if !guard.http.requests.contains_key(&id) {
            return Err(RootedError::host("shared HTTP request has already ended"));
        }
        guard.http.requests.remove(&id).unwrap()
    };
    let trailers_key = context.string_rooted("__quenchTrailers");
    let trailers_root = context.get_property_rooted(request_root, trailers_key)?;
    context.release_root(trailers_key);
    let trailers = read_headers(context, Some(trailers_root), false);
    context.release_root(trailers_root);
    request.trailers = trailers?;
    let socket_key = context.string_rooted("socket");
    let socket_object = context.get_property_rooted(request_root, socket_key)?;
    let supplied_socket = if context.is_object_rooted(socket_object)? {
        property_id(context, socket_object, "__quenchNetSocketId")?
    } else {
        None
    };
    let request_bytes = serialize(&request);
    let state = context.host_mut().shared_state();
    let reusable_socket = supplied_socket.or_else(|| {
        request.agent.and_then(|agent| {
            let host = state.borrow();
            host.http
                .agents
                .get(&agent)?
                .iter()
                .copied()
                .find(|socket| {
                    !host.http.clients.contains_key(socket)
                        && net_shared_vm::is_connected(&host.tcp, *socket)
                        && host
                            .http
                            .agent_endpoints
                            .get(&(agent, *socket))
                            .is_some_and(|(name, port)| {
                                name == &request.host && *port == request.port
                            })
                })
        })
    });
    if reusable_socket.is_none() {
        if let Some(agent) = request.agent {
            let should_queue = {
                let host = state.borrow();
                let active = host
                    .http
                    .agents
                    .get(&agent)
                    .into_iter()
                    .flat_map(|sockets| sockets.iter())
                    .filter(|socket| host.http.clients.contains_key(socket))
                    .count();
                active
                    >= host
                        .http
                        .agent_max_sockets
                        .get(&agent)
                        .copied()
                        .unwrap_or(15)
            };
            if should_queue {
                let request_root = context.retain(request_root)?;
                state
                    .borrow_mut()
                    .http
                    .agent_pending
                    .entry(agent)
                    .or_default()
                    .push_back(super::state::PendingClient {
                        request_id: id,
                        async_id: request.async_id,
                        request_root,
                        request: request_bytes,
                        method: request.method,
                        host: request.host,
                        port: request.port,
                    });
                return Ok(context.undefined());
            }
        }
    }
    let (socket, connected_now) = match reusable_socket {
        Some(socket) => (socket, false),
        None => {
            let result = net_shared_vm::connect(
                &mut state.borrow_mut().tcp,
                request.host.clone(),
                request.port,
            );
            (result.map_err(RootedError::host)?, true)
        }
    };
    let request_root = match context.retain(request_root) {
        Ok(root) => root,
        Err(error) => {
            if connected_now {
                net_shared_vm::close_socket(&mut state.borrow_mut().tcp, socket);
            }
            return Err(error);
        }
    };
    let mut host = state.borrow_mut();
    if !connected_now && net_shared_vm::is_connected(&host.tcp, socket) {
        // A supplied public net.Socket has stopped raw reads while stream
        // delivery owns it. HTTP takes that socket over for this exchange, so
        // restart reads when it is already connected. If it is still
        // connecting, Connected owns both read startup and the single initial
        // HTTP write; queueing bytes here as well would send the request twice.
        if !net_shared_vm::read_active(&host.tcp, socket) {
            net_shared_vm::start_read(&mut host.tcp, socket)
                .map_err(|error| RootedError::host(error.to_string()))?;
        }
        net_shared_vm::write(&mut host.tcp, socket, &request_bytes).map_err(RootedError::host)?;
    }
    if let Some(agent) = request.agent {
        if let Some(sockets) = host.http.agents.get_mut(&agent) {
            sockets.insert(socket);
            host.http
                .agent_endpoints
                .insert((agent, socket), (request.host.clone(), request.port));
        }
    }
    host.http.clients.insert(
        socket,
        super::state::Client {
            request_id: id,
            async_id: request.async_id,
            request_root,
            request: request_bytes,
            agent: request.agent,
            response_parser: super::protocol::ResponseParser::for_method(&request.method),
            response_root: None,
        },
    );
    Ok(context.undefined())
}

pub(crate) fn destroy(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let state = context.host_mut().shared_state();
    let (client, pending_root, cancelled_async_id, next_agent_request) = {
        let mut host = state.borrow_mut();
        let request_async_id = host
            .http
            .requests
            .remove(&id)
            .map(|request| request.async_id);
        let pending = host.http.agent_pending.values_mut().find_map(|queue| {
            let index = queue.iter().position(|pending| pending.request_id == id)?;
            queue
                .remove(index)
                .map(|pending| (pending.request_root, pending.async_id))
        });
        host.http.agent_pending.retain(|_, queue| !queue.is_empty());
        let socket = host
            .http
            .clients
            .iter()
            .find_map(|(socket, client)| (client.request_id == id).then_some(*socket));
        let client = socket.and_then(|socket| {
            let client = host.http.clients.remove(&socket)?;
            let next = client.agent.and_then(|agent| {
                host.http
                    .agent_pending
                    .get_mut(&agent)
                    .and_then(|queue| queue.pop_front())
                    .map(|pending| (agent, pending))
            });
            for sockets in host.http.agents.values_mut() {
                sockets.remove(&socket);
            }
            host.http
                .agent_endpoints
                .retain(|(_, owner), _| *owner != socket);
            net_shared_vm::close_socket(&mut host.tcp, socket);
            Some((client, next))
        });
        let (client, next) = client.map_or((None, None), |(client, next)| (Some(client), next));
        host.http.agent_pending.retain(|_, queue| !queue.is_empty());
        let pending_root = pending.map(|(root, _)| root);
        let cancelled_async_id = pending.map(|(_, async_id)| async_id).or(request_async_id);
        (client, pending_root, cancelled_async_id, next)
    };
    if let Some(root) = pending_root {
        context.release_root(root);
    }
    if let Some(client) = client {
        if let Some(response) = client.response_root {
            let supplied_error = args.get(1).copied().filter(|error| {
                context
                    .rooted_value(*error)
                    .is_some_and(|value| !value.is_undefined() && !value.is_null())
            });
            let error = if let Some(error) = supplied_error {
                error
            } else {
                let error = context.error_rooted("socket hang up")?;
                set_text(context, error, "code", "ECONNRESET")?;
                error
            };
            let destroy_key = context.string_rooted("destroy");
            let destroy = context.get_property_rooted(response, destroy_key)?;
            context.call_rooted(destroy, response, &[error])?;
            context.release_root(destroy);
            context.release_root(destroy_key);
            if supplied_error.is_none() {
                context.release_root(error);
            }
            context.release_root(response);
        }
        context.release_root(client.request_root);
        super::super::async_hooks_shared_vm::complete_callback_context(context, client.async_id)?;
    } else if let Some(async_id) = cancelled_async_id {
        super::super::async_hooks_shared_vm::complete_callback_context(context, async_id)?;
    }
    if let Some((agent, pending)) = next_agent_request {
        let socket = net_shared_vm::connect(
            &mut state.borrow_mut().tcp,
            pending.host.clone(),
            pending.port,
        )
        .map_err(RootedError::host)?;
        let mut host = state.borrow_mut();
        if let Some(sockets) = host.http.agents.get_mut(&agent) {
            sockets.insert(socket);
            host.http
                .agent_endpoints
                .insert((agent, socket), (pending.host.clone(), pending.port));
            host.http.clients.insert(
                socket,
                super::state::Client {
                    request_id: pending.request_id,
                    async_id: pending.async_id,
                    request_root: pending.request_root,
                    request: pending.request,
                    agent: Some(agent),
                    response_parser: super::protocol::ResponseParser::for_method(&pending.method),
                    response_root: None,
                },
            );
        } else {
            net_shared_vm::close_socket(&mut host.tcp, socket);
            context.release_root(pending.request_root);
        }
    }
    Ok(context.undefined())
}

pub(crate) fn abort_exchange(
    context: &mut Context<'_>,
    client: super::state::Client,
) -> Result<(), RootedError> {
    let result = {
        let _scope = super::super::async_hooks_shared_vm::enter_context(
            &context.host_mut().shared_state(),
            client.async_id,
        );
        abort_exchange_stream(context, &client)
    };
    if let Some(response) = client.response_root {
        context.release_root(response);
    }
    context.release_root(client.request_root);
    super::super::async_hooks_shared_vm::complete_callback_context(context, client.async_id)?;
    result
}

fn abort_exchange_stream(
    context: &mut Context<'_>,
    client: &super::state::Client,
) -> Result<(), RootedError> {
    // Once response headers have been delivered, an early transport close is
    // an aborted IncomingMessage. Before headers, it remains a ClientRequest
    // socket hang-up error.
    let error = context.error_rooted(if client.response_root.is_some() {
        "aborted"
    } else {
        "socket hang up"
    })?;
    if let Err(failure) = set_text(context, error, "code", "ECONNRESET") {
        context.release_root(error);
        return Err(failure);
    }
    let stream = client.response_root.unwrap_or(client.request_root);
    let destroy_key = context.string_rooted("destroy");
    let destroy = match context.get_property_rooted(stream, destroy_key) {
        Ok(destroy) => destroy,
        Err(failure) => {
            context.release_root(destroy_key);
            context.release_root(error);
            return Err(failure);
        }
    };
    let result = context.call_rooted(destroy, stream, &[error]);
    context.release_root(destroy);
    context.release_root(destroy_key);
    context.release_root(error);
    match result {
        Ok(value) => {
            context.release_root(value);
            Ok(())
        }
        Err(failure) => Err(failure),
    }
}

pub(crate) fn destroy_response(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let state = context.host_mut().shared_state();
    let client = {
        let mut host = state.borrow_mut();
        let socket = host
            .http
            .clients
            .iter()
            .find_map(|(socket, client)| (client.request_id == id).then_some(*socket));
        socket.and_then(|socket| {
            let client = host.http.clients.remove(&socket)?;
            for sockets in host.http.agents.values_mut() {
                sockets.remove(&socket);
            }
            net_shared_vm::close_socket(&mut host.tcp, socket);
            Some(client)
        })
    };
    if let Some(client) = client {
        if let Some(response) = client.response_root {
            context.release_root(response);
        }
        context.release_root(client.request_root);
    }
    Ok(context.undefined())
}

fn serialize(request: &super::state::OutgoingRequest) -> Vec<u8> {
    let body_len = request.body.len();
    let has_length = request
        .headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-length"));
    let has_chunked = request.headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("transfer-encoding") && value.eq_ignore_ascii_case("chunked")
    });
    let chunked = has_chunked
        || (request.set_default_headers
            && request.body.has_writes()
            && !has_length
            && crate::modules::http_protocol::default_request_chunking(&request.method));
    let host_header = if request.port == 80 {
        request.host.clone()
    } else {
        format!("{}:{}", request.host, request.port)
    };
    let head = crate::modules::http_protocol::request_head_with_chunking(
        &host_header,
        &request.method,
        &request.path,
        &request.headers,
        body_len,
        request.omit_host
            || !request.set_default_headers
            || request
                .headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("host")),
        request.set_default_headers,
        chunked,
    );
    let mut bytes = head.into_bytes();
    if chunked {
        for chunk in request.body.chunks() {
            bytes.extend_from_slice(&crate::modules::http_protocol::chunk_frame(chunk));
        }
        bytes.extend_from_slice(&crate::modules::http_protocol::chunk_terminator(
            &request.trailers,
        ));
    } else {
        bytes.extend_from_slice(&request.body.bytes());
    }
    bytes
}

pub(crate) fn read_headers(
    context: &mut Context<'_>,
    headers: Option<RootId>,
    relaxed: bool,
) -> Result<Vec<(String, String)>, RootedError> {
    let Some(headers) = headers else {
        return Ok(Vec::new());
    };
    let length = numeric_property(context, headers, "length")?
        .filter(|length| length.is_finite() && *length >= 0.0)
        .unwrap_or(0.0) as usize;
    if length % 2 != 0 {
        return Err(RootedError::host("HTTP header pair list is malformed"));
    }
    let mut result = Vec::with_capacity(length / 2);
    for index in (0..length).step_by(2) {
        let name = array_item(context, headers, index)?;
        let value = array_item(context, headers, index + 1)?;
        let name = context.to_string(name)?;
        let value = context.to_string(value)?;
        if name.is_empty()
            || !name
                .chars()
                .all(crate::modules::http_protocol::is_http_token_char)
        {
            let error = context.type_error_rooted("Invalid HTTP header name")?;
            return Err(context.throw(error));
        }
        if !value
            .encode_utf16()
            .all(|unit| crate::modules::http_protocol::valid_header_unit(unit, relaxed))
        {
            let error = context.type_error_rooted("Invalid HTTP header value")?;
            return Err(context.throw(error));
        }
        result.push((name, value));
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

fn argument_id(context: &mut Context<'_>, root: Option<RootId>) -> Result<u64, RootedError> {
    root.and_then(|root| context.rooted_value(root))
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && *id >= 0.0)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("invalid shared HTTP request identifier"))
}

fn numeric_property(
    context: &mut Context<'_>,
    object: RootId,
    name: &str,
) -> Result<Option<f64>, RootedError> {
    if !context.has_own_property_rooted(object, name)? {
        return Ok(None);
    }
    let key = context.string_rooted(name);
    let value = context.get_property_rooted(object, key)?;
    Ok(context.rooted_value(value).and_then(Value::as_number))
}

fn text_property(
    context: &mut Context<'_>,
    object: RootId,
    name: &str,
) -> Result<Option<String>, RootedError> {
    if !context.has_own_property_rooted(object, name)? {
        return Ok(None);
    }
    let key = context.string_rooted(name);
    let value = context.get_property_rooted(object, key)?;
    match context.rooted_value(value) {
        Some(rooted_value) if !rooted_value.is_undefined() && !rooted_value.is_null() => {
            context.to_string(value).map(Some)
        }
        _ => Ok(None),
    }
}

fn object_property(
    context: &mut Context<'_>,
    object: RootId,
    name: &str,
) -> Result<Option<RootId>, RootedError> {
    if !context.has_own_property_rooted(object, name)? {
        return Ok(None);
    }
    let key = context.string_rooted(name);
    let value = context.get_property_rooted(object, key)?;
    Ok(context
        .rooted_value(value)
        .filter(|value| !value.is_undefined() && !value.is_null())
        .map(|_| value))
}

fn property_id(
    context: &mut Context<'_>,
    object: RootId,
    property: &str,
) -> Result<Option<u64>, RootedError> {
    let key = context.string_rooted(property);
    let value = context.get_property_rooted(object, key)?;
    Ok(context
        .rooted_value(value)
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && *id >= 0.0)
        .map(|id| id as u64))
}
