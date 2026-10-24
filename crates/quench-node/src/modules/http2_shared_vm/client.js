((binding, net, tls, Duplex, EventEmitter, diagnosticsChannel, NghttpError, socketSymbol) => {
  'use strict';

  const bodyChunkSentChannel = diagnosticsChannel.channel('http2.client.stream.bodyChunkSent');
  const bodySentChannel = diagnosticsChannel.channel('http2.client.stream.bodySent');
  const streamCreatedChannel = diagnosticsChannel.channel('http2.client.stream.created');
  const streamStartChannel = diagnosticsChannel.channel('http2.client.stream.start');
  const streamErrorChannel = diagnosticsChannel.channel('http2.client.stream.error');
  const streamFinishChannel = diagnosticsChannel.channel('http2.client.stream.finish');
  const streamCloseChannel = diagnosticsChannel.channel('http2.client.stream.close');

  function emitPriorityDeprecation(message) {
    const key = Symbol.for('quench.node.http2PriorityDeprecationEmitted');
    if (globalThis[key]) return;
    globalThis[key] = true;
    process.emitWarning(message, { type: 'DeprecationWarning', code: 'DEP0194' });
  }

  function http2Frame(type, streamId, payload) {
    const frame = Buffer.allocUnsafe(9 + payload.length);
    frame.writeUIntBE(payload.length, 0, 3);
    frame[3] = type;
    frame[4] = 0;
    frame.writeUInt32BE(streamId & 0x7fffffff, 5);
    payload.copy(frame, 9);
    return frame;
  }

  const settingIds = {
    headerTableSize: 1,
    enablePush: 2,
    maxConcurrentStreams: 3,
    initialWindowSize: 4,
    maxFrameSize: 5,
    maxHeaderListSize: 6,
    maxHeaderSize: 6,
    enableConnectProtocol: 8,
  };

  function encodeSettings(settings) {
    if (settings === undefined) settings = {};
    if (settings === null || typeof settings !== 'object' || Array.isArray(settings)) {
      const error = invalidArgumentType('settings', 'an object');
      throw error;
    }
    const values = [];
    for (const [name, id] of Object.entries(settingIds)) {
      if (!Object.hasOwn(settings, name) || settings[name] === undefined) continue;
      let value = settings[name];
      const booleanSetting = name === 'enablePush' || name === 'enableConnectProtocol';
      if (booleanSetting && typeof value !== 'boolean' ||
          !booleanSetting && typeof value !== 'number') {
        const error = new TypeError(`Invalid value for setting "${name}": ${value}`);
        error.code = 'ERR_HTTP2_INVALID_SETTING_VALUE';
        throw error;
      }
      if (typeof value === 'boolean') value = Number(value);
      if (!Number.isInteger(value) || value < 0 || value > 0xffffffff ||
          name === 'initialWindowSize' && value > 0x7fffffff ||
          name === 'maxFrameSize' && (value < 16384 || value > 0xffffff)) {
        const error = new RangeError(`Invalid value for setting "${name}": ${settings[name]}`);
        error.code = 'ERR_HTTP2_INVALID_SETTING_VALUE';
        throw error;
      }
      if (name === 'maxHeaderSize' && settings.maxHeaderListSize !== undefined) continue;
      values.push([id, value]);
    }
    if (settings.customSettings !== undefined) {
      if (settings.customSettings === null || typeof settings.customSettings !== 'object') {
        throw invalidArgumentType('settings.customSettings', 'an object');
      }
      for (const [key, value] of Object.entries(settings.customSettings)) {
        const id = Number(key);
        if (!Number.isInteger(id) || id < 0x10 || id > 0xffff || !Number.isInteger(value) || value < 0 || value > 0xffffffff) {
          const error = new RangeError(`Invalid value for setting "${key}": ${value}`);
          error.code = 'ERR_HTTP2_INVALID_SETTING_VALUE';
          throw error;
        }
        values.push([id, value]);
      }
      if (Object.keys(settings.customSettings).length > 10) {
        const error = new Error('Maximum number of custom settings exceeded');
        error.code = 'ERR_HTTP2_TOO_MANY_CUSTOM_SETTINGS';
        throw error;
      }
    }
    const payload = Buffer.allocUnsafe(values.length * 6);
    values.forEach(([id, value], index) => {
      payload.writeUInt16BE(id, index * 6);
      payload.writeUInt32BE(value, index * 6 + 2);
    });
    return { payload, pairs: values };
  }

  class ClientHttp2Stream extends Duplex {
    get closed() {
      return this._http2Closed || this._readableState.closed || this._writableState.closed;
    }

    sendTrailers(headers) {
      if (this.destroyed || this.closed) {
        const error = new Error('The stream has been destroyed');
        error.code = 'ERR_HTTP2_INVALID_STREAM';
        throw error;
      }
      if (!this._trailersReady) {
        const error = new Error('Trailers are not ready to be sent');
        error.code = 'ERR_HTTP2_TRAILERS_NOT_READY';
        throw error;
      }
      if (this._trailersSent) {
        const error = new Error('Trailers have already been sent');
        error.code = 'ERR_HTTP2_TRAILERS_ALREADY_SENT';
        throw error;
      }
      if (headers === null || typeof headers !== 'object' || Array.isArray(headers)) {
        throw invalidArgumentType('headers', 'an object');
      }
      this._trailersSent = true;
      const pairs = headerPairs(headers).map(([name, value]) => [name.toLowerCase(), value]);
      binding.submitHeaders(this.session._nativeId, this.id, pairs, true);
      this.session._flush();
      const done = this._trailersFinalCallback;
      this._trailersFinalCallback = undefined;
      if (done) done();
    }

    close(code = 0, callback) {
      if (!Number.isInteger(code) || code < 0 || code > 0xffffffff) {
        const error = new RangeError(`The value of "code" is out of range. It must be >= 0 && <= 4294967295. Received ${code}`);
        error.code = 'ERR_OUT_OF_RANGE';
        throw error;
      }
      if (callback !== undefined && typeof callback !== 'function') {
        throw invalidArgumentType('callback', 'function');
      }
      if (this.closed || this.destroyed) return this;
      if (callback) this.once('close', callback);
      const session = this.session;
      if (session && !session.destroyed && session.socket && !session.socket.destroyed) {
        const payload = Buffer.allocUnsafe(4);
        payload.writeUInt32BE(code, 0);
        session.socket.write(http2Frame(0x3, this.id, payload));
      }
      this.rstCode = code;
      this._http2Closed = true;
      let error;
      if (code && this.listenerCount('error')) {
        error = new Error(`Stream closed with error code ${streamErrorCodeName(code)}`);
        error.code = 'ERR_HTTP2_STREAM_ERROR';
        error.errno = code;
      }
      // Let callers install or replace _destroy immediately after close(), as
      // Node's native Http2Stream does before the destroy machinery runs.
      process.nextTick(() => this.destroy(error));
    }

    priority() {
      if (this.destroyed || this.closed) {
        const error = new Error('The stream has been destroyed');
        error.code = 'ERR_HTTP2_INVALID_STREAM';
        throw error;
      }
      emitPriorityDeprecation(
        'http2Stream.priority is longer supported after priority signalling was deprecated in RFC 9113',
      );
      return this;
    }

    setTimeout(msecs, callback) {
      if (typeof msecs !== 'number') {
        const received = typeof msecs === 'string'
          ? `Received type string ('${msecs}')`
          : `Received type ${typeof msecs}`;
        const error = new TypeError(`The "msecs" argument must be of type number. ${received}`);
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      if (callback !== undefined && typeof callback !== 'function') {
        throw invalidArgumentType('callback', 'function');
      }
      if (callback) this.once('timeout', callback);
      clearTimeout(this._timeoutTimer);
      this._timeoutTimer = undefined;
      if (msecs > 0 && !this.closed) {
        this._timeoutTimer = setTimeout(() => {
          this._timeoutTimer = undefined;
          if (!this.closed) this.emit('timeout');
        }, msecs);
      }
      return this;
    }
  }

  const headerPairs = (headers) => {
    const pairs = [];
    if (Array.isArray(headers)) {
      for (const item of headers) {
        if (Array.isArray(item) && item.length >= 2) {
          pairs.push([String(item[0]), String(item[1])]);
        }
      }
    } else if (headers && typeof headers === 'object') {
      for (const [name, value] of Object.entries(headers)) {
        if (Array.isArray(value)) {
          for (const entry of value) pairs.push([String(name), String(entry)]);
        } else {
          pairs.push([String(name), String(value)]);
        }
      }
    }
    return pairs;
  };

  const singleValueHeaders = new Set([
    'content-type', 'user-agent', 'referer', 'authorization',
    'proxy-authorization', 'if-modified-since', 'if-unmodified-since',
    'from', 'location', 'max-forwards',
  ]);

  const validHeaderToken = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
  const requestPseudoHeaders = new Set([
    ':method', ':scheme', ':authority', ':path', ':protocol',
  ]);

  function validateRequestHeaders(headers) {
    for (const name of Object.keys(headers)) {
      if (name.startsWith(':')) {
        if (!requestPseudoHeaders.has(name)) {
          const error = new TypeError(`Invalid HTTP/2 pseudoheader: ${name}`);
          error.code = 'ERR_HTTP2_INVALID_PSEUDOHEADER';
          throw error;
        }
      } else if (!validHeaderToken.test(name)) {
        const error = new TypeError(`Header name must be a valid HTTP token [${name}]`);
        error.code = 'ERR_INVALID_HTTP_TOKEN';
        throw error;
      }
    }
  }

  function validateSingleValueHeaders(headers, strict = true) {
    if (!strict) return;
    const seen = new Set();
    for (const [rawName, value] of Object.entries(headers)) {
      const name = rawName.toLowerCase();
      if (!singleValueHeaders.has(name)) continue;
      if (seen.has(name) || Array.isArray(value)) {
        const error = new TypeError(`Header field "${name}" must only have a single value`);
        error.code = 'ERR_HTTP2_HEADER_SINGLE_VALUE';
        throw error;
      }
      seen.add(name);
    }
  }

  const headerObject = (headers) => {
    const result = Object.create(null);
    for (const pair of headers || []) {
      if (!Array.isArray(pair) || pair.length < 2) continue;
      const name = String(pair[0]);
      const value = String(pair[1]);
      if (Object.hasOwn(result, name)) {
        result[name] = Array.isArray(result[name])
          ? [...result[name], value]
          : [result[name], value];
      } else {
      result[name] = name === ':status' ? Number(value) : value;
      }
    }
    return result;
  };

  function headerListSize(headers) {
    let size = 0;
    for (const [name, value] of headers || []) {
      size += Buffer.byteLength(String(name)) + Buffer.byteLength(String(value)) + 32;
    }
    return size;
  }

  const bytes = (chunk, encoding) => {
    if (typeof chunk === 'string') return Buffer.from(chunk, encoding || 'utf8');
    if (chunk instanceof Uint8Array) return chunk;
    if (ArrayBuffer.isView(chunk)) {
      return new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
    }
    if (chunk instanceof ArrayBuffer) return new Uint8Array(chunk);
    return new TextEncoder().encode(String(chunk));
  };

  function invalidArgumentType(name, expected) {
    const error = new TypeError(`The "${name}" argument must be of type ${expected}.`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    return error;
  }

  function validateConnectionWindowSize(value) {
    if (value === undefined) return;
    if (typeof value !== 'number') {
      throw invalidArgumentType('connectionWindowSize', 'number');
    }
    if (!Number.isInteger(value) || value < 1 || value > 0x7fffffff) {
      const error = new RangeError(
        'The value of "connectionWindowSize" is out of range. It must be >= 1 && <= 2147483647.',
      );
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
  }

  function validateMaxOriginSetSize(value) {
    if (value == null) return;
    if (typeof value !== 'number') {
      const error = new TypeError('The "options.maxOriginSetSize" argument must be of type number.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (Number.isNaN(value) || value < 0) {
      const error = new RangeError(
        'The value of "options.maxOriginSetSize" is out of range. It must be >= 0.',
      );
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
  }

  const streamErrorCodeName = (code) => ({
    0: 'NGHTTP2_NO_ERROR',
    1: 'NGHTTP2_PROTOCOL_ERROR',
    2: 'NGHTTP2_INTERNAL_ERROR',
    3: 'NGHTTP2_FLOW_CONTROL_ERROR',
    4: 'NGHTTP2_SETTINGS_TIMEOUT',
    5: 'NGHTTP2_STREAM_CLOSED',
    6: 'NGHTTP2_FRAME_SIZE_ERROR',
    7: 'NGHTTP2_REFUSED_STREAM',
    8: 'NGHTTP2_CANCEL',
    9: 'NGHTTP2_COMPRESSION_ERROR',
    10: 'NGHTTP2_CONNECT_ERROR',
    11: 'NGHTTP2_ENHANCE_YOUR_CALM',
    12: 'NGHTTP2_INADEQUATE_SECURITY',
    13: 'NGHTTP2_HTTP_1_1_REQUIRED',
  })[code] || `NGHTTP2_ERROR_${code}`;

  // nghttp2's negative library error for NGHTTP2_ERR_PROTO, which is the
  // integer NghttpError expects (as opposed to the positive HTTP/2 frame
  // error constant NGHTTP2_PROTOCOL_ERROR).
  const NGHTTP2_ERR_PROTO = -505;

  function validateRequestOptions(options) {
    if (options === null || typeof options !== 'object') {
      throw invalidArgumentType('options', 'object');
    }
    if (options.parent !== undefined &&
        (typeof options.parent !== 'number' || !Number.isFinite(options.parent))) {
      throw invalidArgumentType('options.parent', 'number');
    }
    for (const name of ['exclusive', 'silent', 'endStream']) {
      if (options[name] !== undefined && typeof options[name] !== 'boolean') {
        throw invalidArgumentType(`options.${name}`, 'boolean');
      }
    }
    if (options.parent !== undefined && options.parent < 0) {
      const error = new RangeError('The value of "options.parent" is out of range. It must be >= 0.');
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
  }

  function connect(authority, options, callback) {
    if (typeof options === 'function') {
      callback = options;
      options = undefined;
    }
    options = options || {};
    validateConnectionWindowSize(options.connectionWindowSize);

    let parsed;
    if (typeof authority === 'string' || authority instanceof URL) {
      parsed = new URL(String(authority));
    } else if (authority && typeof authority === 'object') {
      options = { ...authority, ...options };
      const protocol = options.protocol || 'https:';
      const host = options.host || options.hostname || 'localhost';
      parsed = new URL(`${protocol}//${host}${options.port ? `:${options.port}` : ''}`);
      if (options.path) parsed.pathname = options.path;
    } else {
      throw new TypeError('The "authority" argument must be of type string or an instance of URL.');
    }

    validateMaxOriginSetSize(options.maxOriginSetSize);

    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
      const error = new Error(`The protocol must be either http: or https:. Received '${parsed.protocol}'`);
      error.code = 'ERR_HTTP2_UNSUPPORTED_PROTOCOL';
      throw error;
    }

    const secure = parsed.protocol === 'https:';
    const hostname = options.hostname || parsed.hostname;
    // Preserve the spelling of an explicit URL port. In particular, the
    // public net.connect call for http://host:80 receives the string "80".
    let urlPort;
    if (typeof authority === 'string') {
      const match = /^[a-z][a-z0-9+.-]*:\/\/(?:[^/@]*@)?(?:\[[^\]]+\]|[^/:?#]+)(?::([0-9]+))?/i.exec(authority);
      if (match) urlPort = match[1];
    }
    const explicitPort = options.port !== undefined
      ? options.port
      : (urlPort !== undefined ? urlPort : parsed.port);
    const port = explicitPort || (secure ? 443 : 80);
    const connectionOptions = { ...options, host: hostname, port };
    delete connectionOptions.protocol;
    delete connectionOptions.hostname;
    delete connectionOptions.path;
    delete connectionOptions.createConnection;

    // The native session emits its mandatory initial SETTINGS frame when it
    // first drains. Keep user-provided initial values in that frame rather
    // than queueing a second SETTINGS frame (which also creates an extra ACK).
    const initialSettings = options.settings === undefined
      ? undefined
      : encodeSettings(options.settings);

    let socket;
    if (typeof options.createConnection === 'function') {
      socket = options.createConnection(authority, options);
    } else if (secure) {
      socket = tls.connect({ ...connectionOptions, ALPNProtocols: ['h2'] });
    } else {
      socket = net.connect(connectionOptions);
    }

    const nativeId = binding.newSession(false);
    if (options.connectionWindowSize !== undefined) {
      binding.setLocalWindowSize(nativeId, options.connectionWindowSize);
    }
    const session = new EventEmitter();
    session.socket = socket;
    session[socketSymbol] = socket;
    session.encrypted = secure;
    if (secure) session.originSet = [parsed.origin];
    session._maxOriginSetSize = options.maxOriginSetSize == null
      ? 128
      : options.maxOriginSetSize;
    session.alpnProtocol = secure ? undefined : 'h2c';
    session.connecting = true;
    session.connected = false;
    session.closed = false;
    session.destroyed = false;
    session._nativeId = nativeId;
    session._streams = new Map();
    session._pendingPings = new Map();
    session.maxOutstandingPings = options.maxOutstandingPings || 10;
    session.state = {
      effectiveLocalWindowSize: options.connectionWindowSize ?? 65535,
      localWindowSize: Math.max(65535, options.connectionWindowSize ?? 65535),
      remoteWindowSize: 65535,
    };
    session.localSettings = {
      headerTableSize: 4096, enablePush: true, maxConcurrentStreams: 0xffffffff,
      initialWindowSize: 65535, maxFrameSize: 16384, maxHeaderListSize: 65535,
      maxHeaderSize: 65535, enableConnectProtocol: false, customSettings: Object.create(null),
      ...(options.settings || {}),
    };
    session.localSettings.customSettings = {
      ...(options.settings?.customSettings || {}),
    };
    session._settingsPendingCount = 1;
    session.pendingSettingsAck = session._settingsPendingCount > 0;
    let initialSettingsAcksSeen = 0;
    session.remoteCustomSettings = new Set(options.remoteCustomSettings || []);
    session.remoteSettings = { ...session.localSettings, customSettings: Object.create(null) };
    let serverPrefaceProbe = Buffer.alloc(0);
    let serverPrefaceReceived = false;
    let inboundFrameBuffer = Buffer.alloc(0);
    let nativeClosed = false;
    let initialSettingsFramePending = true;

    const flush = () => {
      if (nativeClosed || !socket || socket.destroyed || socket.writableEnded) return;
      const outbound = binding.drainOutbound(nativeId);
      if (!outbound || !outbound.length || socket.writableEnded) return;
      let data = Buffer.from(outbound);
      // Node enforces maxSendHeaderBlockLength on the encoded HEADERS block
      // before writing it to the transport. Keep the nghttp2 frame out of the
      // socket and report the same local frame/stream errors when the caller
      // opted into a smaller limit.
      const headerBlockLimit = options.maxSendHeaderBlockLength;
      if (Number.isInteger(headerBlockLimit) && headerBlockLimit >= 0) {
        const preface = Buffer.from('PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n');
        let offset = data.length >= preface.length && data.subarray(0, preface.length).equals(preface)
          ? preface.length : 0;
        const chunks = [];
        if (offset) chunks.push(data.subarray(0, offset));
        while (offset + 9 <= data.length) {
          const length = data.readUIntBE(offset, 3);
          const end = offset + 9 + length;
          if (end > data.length) break;
          const type = data[offset + 3];
          const streamId = data.readUInt32BE(offset + 5) & 0x7fffffff;
          const stream = type === 0x1 ? session._streams.get(streamId) : undefined;
          if (stream && length > headerBlockLimit && !stream._maxHeaderBlockErrorReported) {
            stream._maxHeaderBlockErrorReported = true;
            process.nextTick(() => {
              if (stream.destroyed) return;
              stream.emit('frameError', 0x1, 0x6); // HEADERS, FRAME_SIZE_ERROR
              const error = new Error('Stream closed with error code NGHTTP2_REFUSED_STREAM');
              error.code = 'ERR_HTTP2_STREAM_ERROR';
              error.errno = 7;
              stream.rstCode = 7;
              stream.destroy(error);
            });
          } else {
            chunks.push(data.subarray(offset, end));
          }
          offset = end;
        }
        if (offset < data.length) chunks.push(data.subarray(offset));
        data = Buffer.concat(chunks);
      }
      if (initialSettingsFramePending) {
        const preface = Buffer.from('PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n');
        const frameOffset = data.length >= preface.length &&
          data.subarray(0, preface.length).equals(preface) ? preface.length : 0;
        if (data.length >= frameOffset + 9) {
          const frameLength = data.readUIntBE(frameOffset, 3);
          const isInitialSettings = data.length >= frameOffset + 9 + frameLength &&
            data[frameOffset + 3] === 0x4 && data[frameOffset + 4] === 0 &&
            data.readUInt32BE(frameOffset + 5) === 0;
          if (isInitialSettings) {
            initialSettingsFramePending = false;
            if (initialSettings !== undefined) {
              data = Buffer.concat([
                data.subarray(0, frameOffset),
                http2Frame(0x4, 0, initialSettings.payload),
                data.subarray(frameOffset + 9 + frameLength),
              ]);
            }
          }
        }
      }
      if (data.length && !socket.writableEnded) socket.write(data);
    };
    session._flush = flush;

    const finishNative = () => {
      if (nativeClosed) return;
      nativeClosed = true;
      session.closed = true;
      session.destroyed = true;
      binding.closeSession(nativeId);
      for (const stream of session._streams.values()) {
        stream.push(null);
        stream.destroy();
      }
      session._streams.clear();
    };

    const dispatch = (events) => {
      for (const event of events || []) {
        if (event.type === 'push') {
          const headers = headerObject(event.headers);
          const sensitiveHeaders = process.getBuiltinModule('http2').sensitiveHeaders;
          if (sensitiveHeaders) headers[sensitiveHeaders] = [];
          const stream = new ClientHttp2Stream({
            allowHalfOpen: false,
            autoDestroy: false,
            readable: true,
            writable: false,
            read() {},
          });
          stream.id = event.streamId;
          stream.session = session;
          stream.pushStream = true;
          if (streamCreatedChannel.hasSubscribers) {
            streamCreatedChannel.publish({ stream, headers });
          }
          if (streamStartChannel.hasSubscribers) {
            streamStartChannel.publish({ stream, headers });
          }
          stream.once('error', (error) => {
            if (streamErrorChannel.hasSubscribers) streamErrorChannel.publish({ stream, error });
          });
          stream.once('close', () => {
            if (streamCloseChannel.hasSubscribers) streamCloseChannel.publish({ stream });
          });
          session._streams.set(event.streamId, stream);
          session.emit('stream', stream, headers, 4, headers);
        } else if (event.type === 'headers') {
          const stream = session._streams.get(event.streamId);
          if (!stream) continue;
          if (headerListSize(event.headers) > session.localSettings.maxHeaderListSize) {
            const code = 11; // NGHTTP2_ENHANCE_YOUR_CALM
            stream.rstCode = code;
            binding.submitRstStream(nativeId, event.streamId, code);
            flush();
            const error = new Error(`Stream closed with error code ${streamErrorCodeName(code)}`);
            error.code = 'ERR_HTTP2_STREAM_ERROR';
            error.errno = code;
            stream.destroy(error);
            continue;
          }
          const headers = headerObject(event.headers);
          if (secure && headers[':status'] === 421 && stream._requestHeaders?.[':authority']) {
            try {
              const rejectedOrigin = new URL(
                `${parsed.protocol}//${stream._requestHeaders[':authority']}`,
              ).origin;
              session.originSet = session.originSet.filter((origin) => origin !== rejectedOrigin);
            } catch {}
          }
          const status = headers[':status'];
          if (stream._receivedResponseHeaders && status === undefined) {
            if (Object.keys(headers).length) stream.emit('trailers', headers, 0, headers);
            if (event.endStream) {
              stream._responseEnded = true;
              stream.push(null);
            }
          } else if (status >= 100 && status < 200) {
            if (status === 100) stream.emit('continue');
            stream.emit('headers', headers, 0, headers);
          } else {
            stream._receivedResponseHeaders = true;
            if (event.endStream) stream._responseEnded = true;
            if (stream.pushStream) stream.emit('push', headers, 0, headers);
            else stream.emit('response', headers, 0, headers);
            if (streamFinishChannel.hasSubscribers) {
              streamFinishChannel.publish({ stream, headers, flags: 0 });
            }
          }
          if (event.endStream) stream.push(null);
        } else if (event.type === 'data') {
          const stream = session._streams.get(event.streamId);
          if (stream) {
            stream.push(bytes(event.data));
            if (event.endStream) {
              stream._responseEnded = true;
              stream.push(null);
            }
          }
        } else if (event.type === 'close') {
          const stream = session._streams.get(event.streamId);
          if (stream) {
            stream._http2RemoteClosed = true;
            if (!stream._responseEnded && !stream.aborted) {
              stream.aborted = true;
              stream.emit('aborted');
            }
            stream.push(null);
            stream.rstCode = event.code || 0;
            if (event.code && event.code !== 8) {
              const error = new Error(
                `Stream closed with error code ${streamErrorCodeName(event.code)}`,
              );
              error.code = 'ERR_HTTP2_STREAM_ERROR';
              error.errno = event.code;
              stream.destroy(error);
            } else {
              // A protocol-level successful close completes the HTTP/2
              // stream without destroying the JS Duplex. Node exposes that
              // distinction through `closed === true` and
              // `destroyed === false`.
              let closeEmitted = false;
              const emitClose = () => {
                if (closeEmitted || stream.destroyed) return;
                closeEmitted = true;
                if (stream.aborted && !stream.readableEnded && stream.readableLength === 0) {
                  stream._readableState.endEmitted = true;
                  stream.emit('end');
                }
                if (stream.aborted && stream.readableLength === 0 && !stream.readableEnded) {
                  Object.defineProperty(stream, 'readableEnded', {
                    value: true,
                    configurable: true,
                  });
                }
                stream._readableState.closed = true;
                stream.emit('close');
              };
              stream.once('end', emitClose);
              setTimeout(emitClose, 100);
            }
            session._streams.delete(event.streamId);
          }
        } else if (event.type === 'error') {
          const error = new Error(event.message || 'HTTP/2 protocol error');
          error.code = event.code;
          session.emit('error', error);
        }
      }
      flush();
    };

    session.request = (headers = {}, requestOptions = {}) => {
      if (session.closed || session.destroyed) {
        const error = new Error('The session has been destroyed');
        error.code = 'ERR_HTTP2_INVALID_SESSION';
        const stream = new ClientHttp2Stream({
          allowHalfOpen: false,
          autoDestroy: false,
          read() {},
          write(_chunk, _encoding, done) { done(error); },
        });
        stream.session = session;
        stream.rstCode = 0;
        process.nextTick(() => stream.destroy(error));
        return stream;
      }
      validateRequestOptions(requestOptions);
      if (Array.isArray(headers)) {
        const normalizedHeaders = Object.create(null);
        for (let index = 0; index + 1 < headers.length; index += 2) {
          normalizedHeaders[String(headers[index])] = headers[index + 1];
        }
        headers = normalizedHeaders;
      }
      if (requestOptions.weight !== undefined) {
        emitPriorityDeprecation('Priority signaling has been deprecated as of RFC 9113.');
      }
      const requestHeaders = Object.assign(Object.create(null), headers);
      validateRequestHeaders(requestHeaders);
      validateSingleValueHeaders(requestHeaders, options.strictSingleValueFields !== false);
      if (requestHeaders[':method'] === undefined) {
        requestHeaders[':method'] = requestOptions.method || 'GET';
      }
      if (requestHeaders[':authority'] === undefined && requestHeaders.host === undefined) {
        requestHeaders[':authority'] = parsed.host;
      }
      if (requestHeaders[':scheme'] === undefined) {
        requestHeaders[':scheme'] = parsed.protocol.slice(0, -1);
      }
      if (requestHeaders[':path'] === undefined) {
        requestHeaders[':path'] = requestOptions.path || '/';
      }
      // Node's HTTP/2 API accepts ordinary header names with arbitrary case,
      // but HTTP/2 wire field names must be lowercase. Normalize before
      // handing the NV pairs to nghttp2 (pseudo-headers are already lowercase).
      const pairs = headerPairs(requestHeaders)
        .map(([name, value]) => [name.toLowerCase(), value])
        // HTTP/2 requires pseudo-headers to precede every regular header.
        // Defaults inserted above otherwise end up after caller headers such
        // as User-Agent, and nghttp2 responds with PROTOCOL_ERROR.
        .sort(([left], [right]) => Number(right.startsWith(':')) - Number(left.startsWith(':')));
      const method = requestHeaders[':method'];
      const endStream = requestOptions.endStream === undefined
        ? ['DELETE', 'GET', 'HEAD'].includes(method)
        : requestOptions.endStream;
      const streamId = binding.submitRequest(nativeId, pairs, endStream);
      const stream = new ClientHttp2Stream({
        allowHalfOpen: false,
        autoDestroy: false,
        // Keep original strings and encodings for the HTTP/2 body diagnostics
        // channel; convert to bytes only when submitting DATA frames.
        decodeStrings: false,
        writable: !endStream,
        ...requestOptions,
        read() {},
        write(chunk, encoding, done) {
          try {
            const data = bytes(chunk, encoding);
            if (data.byteLength) {
              binding.submitData(nativeId, streamId, data, false);
              flush();
            }
            if (bodyChunkSentChannel.hasSubscribers) {
              bodyChunkSentChannel.publish({ stream: this, writev: false, data: chunk, encoding });
            }
            done();
          } catch (error) {
            done(error);
          }
        },
        writev(chunks, done) {
          try {
            // diagnostics_channel exposes the Writable writev entries, each
            // retaining both its original chunk and encoding. Keep these
            // records intact instead of publishing only the chunk values.
            const data = chunks.every(({ encoding }) => encoding === 'buffer')
              ? chunks.map(({ chunk }) => chunk)
              : chunks;
            const combined = Buffer.concat(chunks.map(({ chunk, encoding }) => bytes(chunk, encoding)));
            if (combined.length) {
              binding.submitData(nativeId, streamId, combined, false);
              flush();
            }
            if (bodyChunkSentChannel.hasSubscribers) {
              bodyChunkSentChannel.publish({ stream: this, writev: true, data, encoding: '' });
            }
            done();
          } catch (error) {
            done(error);
          }
        },
        final(done) {
          try {
            if (this._http2RemoteClosed) {
              done();
              return;
            }
            if (requestOptions.waitForTrailers) {
              this._trailersReady = true;
              this._trailersFinalCallback = done;
              if (!this.emit('wantTrailers')) this.sendTrailers({});
              return;
            }
            if (!endStream) {
              binding.submitData(nativeId, streamId, new Uint8Array(0), true);
              flush();
            }
            if (bodySentChannel.hasSubscribers) bodySentChannel.publish({ stream: this });
            done();
          } catch (error) {
            done(error);
          }
        },
        destroy(error, done) {
          clearTimeout(this._timeoutTimer);
          this._timeoutTimer = undefined;
          session._streams.delete(streamId);
          if (error && this.rstCode === 0) {
            this.rstCode = 2; // NGHTTP2_INTERNAL_ERROR
            if (socket && !socket.destroyed && !socket.writableEnded) {
              binding.submitRstStream(nativeId, streamId, this.rstCode);
              flush();
            }
          }
          if (error) done(error);
          else done();
        },
      });
      stream.id = streamId;
      stream.session = session;
      stream.rstCode = 0;
      stream._requestHeaders = requestHeaders;
      if (streamCreatedChannel.hasSubscribers) {
        streamCreatedChannel.publish({ stream, headers: requestHeaders });
      }
      if (streamStartChannel.hasSubscribers) {
        streamStartChannel.publish({ stream, headers: requestHeaders });
      }
      stream.once('error', (error) => {
        if (error?.name === 'AbortError' || error?.code === 'ABORT_ERR') stream.rstCode = 8;
        if (streamErrorChannel.hasSubscribers) streamErrorChannel.publish({ stream, error });
      });
      stream.once('close', () => {
        if (streamCloseChannel.hasSubscribers) streamCloseChannel.publish({ stream });
      });
      if (!session.connected) {
        stream._http2PendingConnectCork = true;
        stream.cork();
      }
      session._streams.set(streamId, stream);
      flush();
      if (session.connected) process.nextTick(() => {
        if (!stream.destroyed) stream.emit('ready');
      });
      return stream;
    };

    session.close = (callback) => {
      if (callback) session.once('close', callback);
      if (session.closed) return session;
      session.closed = true;
      if (session.connecting) {
        const error = new Error('The session was closed before the connection was established');
        error.code = 'ERR_HTTP2_GOAWAY_SESSION';
        for (const stream of [...session._streams.values()]) stream.destroy(error);
      }
      if (socket && !socket.destroyed) socket.end();
      else finishNative();
      return session;
    };

    session.settings = (settings, callback) => {
      if (callback !== undefined && typeof callback !== 'function') {
        throw invalidArgumentType('callback', 'a function');
      }
      const { pairs } = encodeSettings(settings);
      if (callback) session.once('localSettings', callback);
      session._settingsPendingCount++;
      session.pendingSettingsAck = true;
      if (session._settingsPendingCount > (options.maxOutstandingSettings || 10)) {
        const error = new Error('Maximum number of outstanding settings frames');
        error.code = 'ERR_HTTP2_MAX_PENDING_SETTINGS_ACK';
        session.emit('error', error);
        session._settingsOverflow = true;
        session.destroyed = true;
        if (socket && !socket.destroyed) socket.destroy();
        return undefined;
      }
      binding.submitSettings(nativeId, pairs);
      flush();
      session.localSettings = { ...session.localSettings, ...settings };
      session.localSettings.customSettings = { ...(settings.customSettings || session.localSettings.customSettings) };
      return undefined;
    };

    session.setTimeout = (msecs, callback) => {
      if (typeof msecs !== 'number') throw invalidArgumentType('msecs', 'number');
      if (callback !== undefined && typeof callback !== 'function') {
        throw invalidArgumentType('callback', 'function');
      }
      if (callback) session.once('timeout', callback);
      if (typeof socket.setTimeout === 'function') socket.setTimeout(msecs);
      return session;
    };

    session.setLocalWindowSize = (windowSize) => {
      if (typeof windowSize !== 'number') {
        const received = windowSize === null
          ? 'Received null'
          : windowSize !== null && typeof windowSize === 'object'
          ? `Received an instance of ${windowSize.constructor?.name || 'Object'}`
          : typeof windowSize === 'function'
          ? `Received function ${windowSize.name}`
          : `Received type ${typeof windowSize} (${String(windowSize)})`;
        const error = new TypeError(`The "windowSize" argument must be of type number. ${received}`);
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      if (!Number.isInteger(windowSize) || windowSize < 0 || windowSize > 0x7fffffff) {
        const error = new RangeError(`The value of "windowSize" is out of range. It must be >= 0 && <= 2147483647. Received ${windowSize}`);
        error.code = 'ERR_OUT_OF_RANGE';
        throw error;
      }
      const previous = session.state.localWindowSize;
      session.state.effectiveLocalWindowSize = windowSize;
      if (windowSize > previous) {
        const payload = Buffer.allocUnsafe(4);
        payload.writeUInt32BE(windowSize - previous, 0);
        if (socket && !socket.destroyed && !socket.writableEnded) {
          socket.write(http2Frame(0x8, 0, payload));
        }
        session.state.localWindowSize = windowSize;
      }
    };

    session.ping = (payload, callback) => {
      if (typeof payload === 'function') {
        callback = payload;
        payload = undefined;
      }
      if (payload === undefined) payload = Buffer.alloc(8);
      if (!ArrayBuffer.isView(payload)) {
        const received = payload === null ? 'Received null'
          : typeof payload === 'number' ? `Received type number (${payload})`
          : typeof payload === 'string' ? `Received type string ('${payload}')`
          : typeof payload === 'boolean' ? `Received type boolean (${payload})`
          : `Received an instance of ${payload?.constructor?.name || 'Object'}`;
        const error = new TypeError(
          `The "payload" argument must be an instance of Buffer, TypedArray, or DataView. ${received}`,
        );
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      const view = bytes(payload);
      if (view.byteLength !== 8) {
        const error = new RangeError('HTTP2 ping payload must be 8 bytes');
        error.code = 'ERR_HTTP2_PING_LENGTH';
        throw error;
      }
      if (callback !== undefined && typeof callback !== 'function') {
        throw invalidArgumentType('callback', 'function');
      }
      const asyncResource = callback
        ? new (process.getBuiltinModule('async_hooks').AsyncResource)('HTTP2PING')
        : undefined;
      const cancel = () => {
        if (!callback) return;
        const error = new Error('HTTP2 ping cancelled');
        error.code = 'ERR_HTTP2_PING_CANCEL';
        process.nextTick(() => {
          try { asyncResource.runInAsyncScope(callback, undefined, error); }
          finally { asyncResource.emitDestroy(); }
        });
      };
      if (session.destroyed || session.closed || !socket || socket.destroyed || socket.writableEnded) {
        cancel();
        return false;
      }
      if (session._pendingPings.size >= session.maxOutstandingPings) {
        cancel();
        return false;
      }
      const data = Buffer.from(view);
      const key = data.toString('hex');
      session._pendingPings.set(key, {
        callback, asyncResource, started: Date.now(), payload: data,
        result: Buffer.from(payload.buffer, payload.byteOffset, payload.byteLength),
      });
      socket.write(http2Frame(0x6, 0, data));
      return true;
    };

    session.ref = () => {
      if (typeof socket.ref === 'function') socket.ref();
      return session;
    };

    session.unref = () => {
      if (typeof socket.unref === 'function') socket.unref();
      return session;
    };

    session.destroy = (error) => {
      if (session.destroyed) return session;
      session.destroyed = true;
      if (error && session.listenerCount('error')) session.emit('error', error);
      if (socket && !socket.destroyed) socket.destroy(error);
      else finishNative();
      return session;
    };

    socket.on('data', (chunk) => {
      if (session._originSetOverflow) return;
      const originsToEmit = [];
      const altsvcToEmit = [];
      const goawaysToEmit = [];
      let eventsForDispatch = [];
      try {
        const input = bytes(chunk);
        // ORIGIN is an HTTP/2 extension frame. nghttp2 accepts it as an
        // unknown extension, so decode it here to provide Node's session
        // 'origin' event while leaving normal frame processing native.
        inboundFrameBuffer = Buffer.concat([inboundFrameBuffer, Buffer.from(input)]);
        const framesForNative = [];
        while (inboundFrameBuffer.length >= 9) {
          const frameLength = inboundFrameBuffer.readUIntBE(0, 3);
          if (inboundFrameBuffer.length < 9 + frameLength) break;
          const frameType = inboundFrameBuffer[3];
          const streamId = inboundFrameBuffer.readUInt32BE(5) & 0x7fffffff;
          if (frameType === 0x8 && streamId === 0 && frameLength === 4) {
            session.state.remoteWindowSize +=
              inboundFrameBuffer.readUInt32BE(9) & 0x7fffffff;
            framesForNative.push(inboundFrameBuffer.subarray(0, 9 + frameLength));
          } else if (frameType === 0x4 && streamId === 0 && (inboundFrameBuffer[4] & 1) === 0 && frameLength % 6 === 0) {
            const payload = inboundFrameBuffer.subarray(9, 9 + frameLength);
            const remote = { ...session.remoteSettings, customSettings: Object.create(null) };
            for (let offset = 0; offset < payload.length; offset += 6) {
              const id = payload.readUInt16BE(offset);
              const value = payload.readUInt32BE(offset + 2);
              const name = Object.keys(settingIds).find((key) => settingIds[key] === id && key !== 'maxHeaderSize');
              if (name) remote[name] = id === 2 || id === 8 ? value !== 0 : value;
              else if (id >= 0x10 && session.remoteCustomSettings.has(id)) remote.customSettings[id] = value;
            }
            session.remoteSettings = remote;
            session.emit('remoteSettings', remote);
            framesForNative.push(inboundFrameBuffer.subarray(0, 9 + frameLength));
          } else if (frameType === 0x4 && streamId === 0 && (inboundFrameBuffer[4] & 1) !== 0) {
            initialSettingsAcksSeen++;
            session._settingsPendingCount = Math.max(0, session._settingsPendingCount - 1);
            session.pendingSettingsAck = session._settingsPendingCount > 0;
            if (!options.settings || initialSettingsAcksSeen !== 2) {
              session.emit('localSettings', session.localSettings);
            }
            framesForNative.push(inboundFrameBuffer.subarray(0, 9 + frameLength));
          } else if (frameType === 0x0a) {
            const payload = inboundFrameBuffer.subarray(9, 9 + frameLength);
            let origin = '';
            let offset = 0;
            if (streamId === 0 && payload.length >= 2) {
              const originLength = payload.readUInt16BE(0);
              offset = 2;
              if (offset + originLength <= payload.length) {
                origin = payload.toString('utf8', offset, offset + originLength);
                offset += originLength;
              }
            }
            altsvcToEmit.push({
              alt: payload.toString('utf8', offset),
              origin,
              streamId,
            });
          } else if (frameType === 0x0c && streamId === 0) {
            const payload = inboundFrameBuffer.subarray(9, 9 + frameLength);
            const origins = [];
            let offset = 0;
            while (offset + 2 <= payload.length) {
              const length = payload.readUInt16BE(offset);
              offset += 2;
              if (offset + length > payload.length) break;
              origins.push(payload.toString('utf8', offset, offset + length));
              offset += length;
            }
            if (origins.length && secure) originsToEmit.push(origins);
          } else if (frameType === 0x6 && streamId === 0 && frameLength === 8) {
            const payload = inboundFrameBuffer.subarray(9, 17);
            if (inboundFrameBuffer[4] & 1) {
              const key = payload.toString('hex');
              const pending = session._pendingPings.get(key);
              if (pending) {
                session._pendingPings.delete(key);
                if (pending.callback) {
                  try {
                    pending.asyncResource.runInAsyncScope(
                      pending.callback, undefined, null, Date.now() - pending.started,
                      pending.result,
                    );
                  } finally {
                    pending.asyncResource.emitDestroy();
                  }
                }
              }
            } else {
              session.emit('ping', Buffer.from(payload));
              framesForNative.push(inboundFrameBuffer.subarray(0, 9 + frameLength));
            }
          } else if (inboundFrameBuffer[3] === 0x7 && inboundFrameBuffer.readUInt32BE(5) === 0 && frameLength >= 8) {
            const payload = inboundFrameBuffer.subarray(9, 9 + frameLength);
            goawaysToEmit.push({
              code: payload.readUInt32BE(4),
              lastStreamID: payload.readUInt32BE(0) & 0x7fffffff,
              opaqueData: Buffer.from(payload.subarray(8)),
            });
          } else {
            framesForNative.push(inboundFrameBuffer.subarray(0, 9 + frameLength));
          }
          inboundFrameBuffer = inboundFrameBuffer.subarray(9 + frameLength);
        }
        if (!serverPrefaceReceived) {
          const probeLength = Math.min(9, serverPrefaceProbe.length + input.length);
          if (serverPrefaceProbe.length < probeLength) {
            serverPrefaceProbe = Buffer.concat([
              serverPrefaceProbe,
              Buffer.from(input).subarray(0, probeLength - serverPrefaceProbe.length),
            ]);
          }
          if (serverPrefaceProbe.length >= 9) {
            // The first frame from an HTTP/2 server must be SETTINGS on the
            // connection stream. nghttp2 can consume an HTTP/1 error response
            // without surfacing a parser error, leaving request/session
            // listeners waiting forever; reject that invalid preface here.
            if (serverPrefaceProbe[3] !== 0x4 ||
                (serverPrefaceProbe[5] & 0x80) !== 0 ||
                serverPrefaceProbe[5] !== 0 ||
                serverPrefaceProbe[6] !== 0 ||
                serverPrefaceProbe[7] !== 0 ||
                serverPrefaceProbe[8] !== 0) {
              throw new NghttpError(NGHTTP2_ERR_PROTO);
            }
            serverPrefaceReceived = true;
            serverPrefaceProbe = Buffer.alloc(0);
          }
        }
        if (framesForNative.length) {
          try {
            eventsForDispatch = binding.feed(nativeId, Buffer.concat(framesForNative));
          } catch (error) {
            if (error instanceof NghttpError) throw error;
            throw new NghttpError(NGHTTP2_ERR_PROTO);
          }
        }
      } catch (error) {
        if (!(error instanceof NghttpError)) throw error;
        const protocolError = error instanceof NghttpError
          ? error
          : new NghttpError(NGHTTP2_ERR_PROTO);
        for (const stream of [...session._streams.values()]) {
          if (stream.listenerCount('error')) {
            // Deliver the stream failure before the connection-level error,
            // matching Node's HTTP/2 teardown ordering. `destroy(error)` in
            // this host can otherwise defer the stream event until after the
            // session closes.
            stream.emit('error', protocolError);
            stream.destroy();
          } else {
            stream.destroy(protocolError);
          }
        }
        if (session.listenerCount('error')) session.emit('error', protocolError);
        session.destroy();
        return;
      }
      for (const origins of originsToEmit) {
        const nextOriginSet = new Set([...(session.originSet || []), ...origins]);
        if (nextOriginSet.size > session._maxOriginSetSize) {
          const error = new Error('The session has exceeded its maximum origin set size');
          error.code = 'ERR_HTTP2_TOO_MANY_ORIGINS';
          session._originSetOverflow = true;
          session.emit('error', error);
          session.close();
          break;
        }
        for (const origin of origins) {
          if (!session.originSet.includes(origin)) session.originSet.push(origin);
        }
        session.emit('origin', origins);
      }
      for (const { alt, origin, streamId } of altsvcToEmit) {
        session.emit('altsvc', alt, origin, streamId);
      }
      for (const { code, lastStreamID, opaqueData } of goawaysToEmit) {
        session.emit('goaway', code, lastStreamID, opaqueData);
        if (code !== 0) {
          const error = new Error(`The HTTP/2 session has been shut down with error code ${streamErrorCodeName(code)}`);
          error.code = 'ERR_HTTP2_SESSION_ERROR';
          error.errno = code;
          for (const stream of [...session._streams.values()]) {
            if (stream.listenerCount('error')) stream.emit('error', error);
            stream.destroy();
          }
          if (session.listenerCount('error')) session.emit('error', error);
        }
      }
      dispatch(eventsForDispatch);
    });
    let connectEmitted = false;
    const onSocketConnect = () => {
      if (connectEmitted) return;
      connectEmitted = true;
      session.connecting = false;
      session.connected = true;
      flush();
      session.emit('connect', session, socket);
      if (callback) callback(session, socket);
      for (const stream of session._streams.values()) {
        if (stream._http2PendingConnectCork) {
          stream._http2PendingConnectCork = false;
          stream.uncork();
        }
        process.nextTick(() => {
          if (!stream.destroyed) stream.emit('ready');
        });
      }
    };
    socket.once('connect', onSocketConnect);
    // A caller-provided Duplex may already be connected and never emits the
    // net.Socket `connect` event. HTTP/2 sessions over such streams still
    // become usable immediately.
    if (!socket.connecting && !socket.secureConnecting) process.nextTick(onSocketConnect);
    socket.on('error', (error) => {
      if (session._settingsOverflow || session._originSetOverflow) return;
      if (error.code === 'EPIPE') {
        const sessionError = new Error('Session closed with error code 2');
        sessionError.code = 'ERR_HTTP2_SESSION_ERROR';
        error = sessionError;
      }
      if (session.listenerCount('error')) session.emit('error', error);
    });
    socket.on('timeout', () => session.emit('timeout'));
    socket.once('close', () => {
      finishNative();
      session.emit('close');
    });

    flush();
    return session;
  }

  Object.defineProperty(connect, Symbol.for('nodejs.util.promisify.custom'), {
    value(authority, options) {
      return new Promise((resolve, reject) => {
        let session;
        try {
          session = connect(authority, options);
        } catch (error) {
          reject(error);
          return;
        }
        const onError = (error) => {
          session.removeListener('connect', onConnect);
          reject(error);
        };
        const onConnect = () => {
          session.removeListener('error', onError);
          resolve(session);
        };
        session.once('connect', onConnect);
        session.once('error', onError);
      });
    },
  });

  return { connect };
})
