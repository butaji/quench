((binding, net, tls, Duplex, EventEmitter, compat, diagnosticsChannel, NghttpError, socketSymbol) => {
  'use strict';

  const streamCreatedChannel = diagnosticsChannel.channel('http2.server.stream.created');
  const streamStartChannel = diagnosticsChannel.channel('http2.server.stream.start');
  const streamErrorChannel = diagnosticsChannel.channel('http2.server.stream.error');
  const streamFinishChannel = diagnosticsChannel.channel('http2.server.stream.finish');
  const streamCloseChannel = diagnosticsChannel.channel('http2.server.stream.close');

  function emitPriorityDeprecation() {
    const key = Symbol.for('quench.node.http2PriorityDeprecationEmitted');
    if (globalThis[key]) return;
    globalThis[key] = true;
    process.emitWarning(
      'http2Stream.priority is longer supported after priority signalling was deprecated in RFC 9113',
      { type: 'DeprecationWarning', code: 'DEP0194' },
    );
  }

  function objectHeaders(pairs) {
    const headers = Object.create(null);
    const rawHeaders = [];
    for (const pair of pairs) {
      const name = String(pair[0]);
      const value = String(pair[1]);
      rawHeaders.push(name, value);
      if (Object.hasOwn(headers, name)) {
        const previous = headers[name];
        if (name === 'set-cookie') {
          headers[name] = Array.isArray(previous) ? [...previous, value] : [previous, value];
        } else {
          const separator = name === 'cookie' ? '; ' : ', ';
          headers[name] = `${previous}${separator}${value}`;
        }
      } else {
        headers[name] = value;
      }
    }
    return { headers, rawHeaders };
  }

  function headerListSize(headers) {
    let size = 0;
    for (const [name, value] of headers || []) {
      size += Buffer.byteLength(String(name)) + Buffer.byteLength(String(value)) + 32;
    }
    return size;
  }

  function validateConnectionWindowSize(value) {
    if (value === undefined) return;
    if (typeof value !== 'number') {
      const error = new TypeError('The "connectionWindowSize" argument must be of type number.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (!Number.isInteger(value) || value < 1 || value > 0x7fffffff) {
      const error = new RangeError(
        'The value of "connectionWindowSize" is out of range. It must be >= 1 && <= 2147483647.',
      );
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
  }

  const fs = process.getBuiltinModule('fs');
  const http = process.getBuiltinModule('http');
  const Buffer = process.getBuiltinModule('buffer').Buffer;
  const inspect = process.getBuiltinModule('util').inspect;

  function invalidFileOption(name, expected, value) {
    const error = new TypeError(
      `The property 'options.${name}' is invalid. Received ${inspect(value)}`,
    );
    error.code = 'ERR_INVALID_ARG_VALUE';
    return error;
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
    headerTableSize: 1, enablePush: 2, maxConcurrentStreams: 3,
    initialWindowSize: 4, maxFrameSize: 5, maxHeaderListSize: 6,
    maxHeaderSize: 6, enableConnectProtocol: 8,
  };

  function encodeSettings(settings) {
    if (settings === undefined) settings = {};
    if (settings === null || typeof settings !== 'object' || Array.isArray(settings)) {
      const error = new TypeError('The "settings" argument must be of type object.');
      error.code = 'ERR_INVALID_ARG_TYPE';
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
      if (typeof value !== 'number') {
        const error = new TypeError(`The "settings.${name}" argument must be of type number.`);
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
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
      if (!settings.customSettings || typeof settings.customSettings !== 'object') {
        const error = new TypeError('The "settings.customSettings" argument must be of type object.');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
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

  const singleValueHeaders = new Set([
    'content-type', 'user-agent', 'referer', 'authorization',
    'proxy-authorization', 'if-modified-since', 'if-unmodified-since',
    'from', 'location', 'max-forwards',
  ]);
  const validHeaderToken = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;

  function validateResponseHeaderNames(headers, allowStatus = true) {
    for (const name of Object.keys(headers)) {
      if (name.startsWith(':')) {
        if (!allowStatus || name !== ':status') {
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

  function validHeaderValue(value) {
    for (const character of String(value)) {
      const code = character.codePointAt(0);
      if (code !== 9 && (code < 0x20 || code === 0x7f || code > 0xff)) return false;
    }
    return true;
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

  function streamErrorCodeName(code) {
    return ({
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
  }

  function receivedArgument(value) {
    if (value === null || value === undefined) return `Received ${value}`;
    if (typeof value === 'function') return `Received function ${value.name}`;
    if (typeof value === 'object') {
      const name = value.constructor?.name;
      return name ? `Received an instance of ${name}` : `Received ${inspect(value)}`;
    }
    return `Received type ${typeof value} (${inspect(value)})`;
  }

  function validateFileOptions(options) {
    if (options === null || typeof options !== 'object') {
      const error = new TypeError('The "options" argument must be of type object.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    for (const [name, type] of [['offset', 'number'], ['length', 'number'], ['statCheck', 'function']]) {
      if (options[name] !== undefined && typeof options[name] !== type) {
        throw invalidFileOption(name, type, options[name]);
      }
    }
  }

  class ServerHttp2Stream extends Duplex {
    constructor(session, id, headers, rawHeaders) {
      super({ allowHalfOpen: true, autoDestroy: false });
      this.id = id;
      this.session = session;
      this.headers = headers;
      this.rawHeaders = rawHeaders;
      this.sentHeaders = undefined;
      this.aborted = false;
      this.rstCode = 0;
      this.headRequest = headers[':method'] === 'HEAD';
      this._h2Closed = false;
      this._responseSent = false;
      this._endSent = false;
      // Node's native Http2Stream inspection exposes these aliases and the
      // nghttp2 stream state record. Keep them enumerable so util.inspect()
      // presents the same stable summary as Node's native stream wrapper.
      this.state = {
        state: 2,
        weight: 16,
        sumDependencyWeight: 0,
        localClose: 0,
        remoteClose: 0,
        localWindowSize: 65535,
      };
      this.readableState = this._readableState;
      this.writableState = this._writableState;
    }

    get closed() { return this._h2Closed || this._readableState.closed || this._writableState.closed; }
    get headersSent() { return this._responseSent; }
    get pushAllowed() { return this.session.remoteSettings.enablePush !== false; }

    _read() {}

    _destroy(error, callback) {
      this._h2Closed = true;
      if (error && this.rstCode === 0) {
        this.rstCode = 2;
        this._sendReset(this.rstCode);
      }
      clearTimeout(this._timeoutTimer);
      this._timeoutTimer = undefined;
      callback(error);
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
        const error = new TypeError('The "callback" argument must be of type function.');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
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

    priority() {
      if (this.destroyed || this.closed) {
        const error = new Error('The stream has been destroyed');
        error.code = 'ERR_HTTP2_INVALID_STREAM';
        throw error;
      }
      emitPriorityDeprecation();
      return this;
    }

    _ensureResponse() {
      // Node implicitly starts a default 200 response when a server stream is
      // written or ended before respond() is called. DATA cannot legally be
      // the first frame on a response stream, so submit the response HEADERS
      // before handing body chunks to nghttp2.
      if (!this._responseSent) this.respond({ ':status': 200 });
    }

    _write(chunk, encoding, callback) {
      try {
        this._ensureResponse();
        const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk, encoding);
        // An empty Writable chunk has no HTTP/2 DATA frame representation.
        // Submitting a zero-length, non-final provider can be interpreted by
        // nghttp2 as end-of-source and prematurely terminate the response.
        if (bytes.length) {
          binding.submitData(this.session._id, this.id, bytes, false);
          this.session._flush();
        }
        // Keep the chunk in Writable's queue until the next turn, matching
        // the native HTTP/2 stream's asynchronous write completion. In
        // particular, compat.ServerResponse proxies writableLength and
        // writableNeedDrain directly from this stream.
        process.nextTick(callback);
      } catch (error) {
        process.nextTick(callback, error);
      }
    }

    _final(callback) {
      const finish = () => {
        try {
          this._ensureResponse();
          if (this._waitForTrailers) {
            this._trailersReady = true;
            this._trailersFinalCallback = callback;
            if (!this.emit('wantTrailers')) this.sendTrailers({});
            return;
          }
          if (!this._endSent) {
            this._endSent = true;
            binding.submitData(this.session._id, this.id, Buffer.alloc(0), true);
            this.session._flush();
          }
          callback();
        } catch (error) {
          callback(error);
        }
      };
      // A server may call pushStream() immediately after end(). Let the
      // synchronous PUSH_PROMISE submission happen before the parent's
      // end-of-stream DATA frame closes it in nghttp2.
      if (this.session.type === 'server') process.nextTick(finish);
      else finish();
    }

    respond(headers = {}, options = {}) {
      if (this.destroyed || this.closed) {
        const error = new Error('The stream has been destroyed');
        error.code = 'ERR_HTTP2_INVALID_STREAM';
        throw error;
      }
      if (this._responseSent) {
        const error = new Error('Response has already been initiated.');
        error.code = 'ERR_HTTP2_HEADERS_SENT';
        throw error;
      }
      if (headers === null || typeof headers !== 'object')
        throw new TypeError('The "headers" argument must be of type object.');
      const values = { ...headers };
      validateResponseHeaderNames(values);
      validateSingleValueHeaders(values, this.session.server._options.strictSingleValueFields !== false);
      if (values[':status'] === undefined) values[':status'] = 200;
      const statusCode = Number(values[':status']) | 0;
      if (statusCode < 200 || statusCode > 599) {
        const error = new RangeError(`Invalid status code: ${statusCode}`);
        error.code = 'ERR_HTTP2_STATUS_INVALID';
        throw error;
      }
      values[':status'] = statusCode;
      const waitForTrailers = options.waitForTrailers === true;
      const endStream = !waitForTrailers && (options.endStream === true ||
        statusCode === 204 || statusCode === 205 || statusCode === 304 || this.headRequest);
      if (options.sendDate !== false && !Object.hasOwn(values, 'date')) {
        values.date = new Date().toUTCString();
      }
      const pairs = [];
      for (const [rawName, value] of Object.entries(values)) {
        const name = rawName.startsWith(':') ? rawName : rawName.toLowerCase();
        if (Array.isArray(value)) {
          for (const item of value) if (validHeaderValue(item)) pairs.push([name, String(item)]);
        } else if (validHeaderValue(value)) {
          pairs.push([name, String(value)]);
        }
      }
      // HTTP/2 requires all pseudo-headers to precede regular headers. A
      // caller's object insertion order can put :status after content-type.
      pairs.sort(([left], [right]) => Number(right.startsWith(':')) - Number(left.startsWith(':')));
      this.sentHeaders = values;
      this._responseSent = true;
      this._waitForTrailers = waitForTrailers;
      this._endSent = endStream;
      binding.submitHeaders(this.session._id, this.id, pairs, endStream);
      this.session._flush();
      if (streamFinishChannel.hasSubscribers) {
        streamFinishChannel.publish({ stream: this, headers: values, flags: 0 });
      }
      if (endStream) this.end();
      return this;
    }

    additionalHeaders(headers) {
      if (this.destroyed || this.closed) {
        const error = new Error('The stream has been destroyed');
        error.code = 'ERR_HTTP2_INVALID_STREAM';
        throw error;
      }
      if (this.headersSent) {
        const error = new Error('Cannot send additional headers after response headers have been sent');
        error.code = 'ERR_HTTP2_HEADERS_AFTER_RESPOND';
        throw error;
      }
      if (headers === null || typeof headers !== 'object' || Array.isArray(headers)) {
        throw new TypeError('The "headers" argument must be of type object.');
      }
      const values = Object.assign(Object.create(null), headers);
      validateResponseHeaderNames(values);
      validateSingleValueHeaders(values, this.session.server._options.strictSingleValueFields !== false);
      if (values[':status'] != null) {
        const statusCode = Number(values[':status']) | 0;
        if (statusCode === 101) {
          const error = new Error('The status code 101 is not supported by HTTP/2');
          error.code = 'ERR_HTTP2_STATUS_101';
          throw error;
        }
        if (statusCode < 100 || statusCode >= 200) {
          const error = new RangeError(`Invalid informational status code: ${statusCode}`);
          error.code = 'ERR_HTTP2_INVALID_INFO_STATUS';
          throw error;
        }
        values[':status'] = statusCode;
      }
      const pairs = [];
      for (const [rawName, value] of Object.entries(values)) {
        const name = rawName.startsWith(':') ? rawName : rawName.toLowerCase();
        if (Array.isArray(value)) {
          for (const item of value) if (validHeaderValue(item)) pairs.push([name, String(item)]);
        } else if (validHeaderValue(value)) {
          pairs.push([name, String(value)]);
        }
      }
      pairs.sort(([left], [right]) => Number(right.startsWith(':')) - Number(left.startsWith(':')));
      binding.submitHeaders(this.session._id, this.id, pairs, false);
      this.session._flush();
    }

    respondWithFile(path, headers = {}, options = {}) {
      this._respondWithFile((callback) => fs.readFile(path, callback), path, headers, options);
      return this;
    }

    respondWithFD(fd, headers = {}, options = {}) {
      const rawFd = fd && typeof fd === 'object' ? fd.fd : fd;
      if (typeof rawFd !== 'number') {
        const error = new TypeError(
          `The "fd" argument must be of type number or an instance of FileHandle. ${receivedArgument(fd)}`,
        );
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      this._respondWithFile((callback) => fs.readFile(rawFd, callback), rawFd, headers, options, true);
      return this;
    }

    _respondWithFile(read, target, headers, options, fromFd = false) {
      if (this.closed || this.destroyed) {
        const error = new Error('The stream has been destroyed');
        error.code = 'ERR_HTTP2_INVALID_STREAM';
        throw error;
      }
      if (this.headersSent) {
        const error = new Error('Response has already been initiated.');
        error.code = 'ERR_HTTP2_HEADERS_SENT';
        throw error;
      }
      validateFileOptions(options);
      if (headers === null || typeof headers !== 'object') {
        const error = new TypeError('The "headers" argument must be of type object.');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      const responseHeaders = { ...headers };
      const statusCode = Number(responseHeaders[':status'] ?? 200);
      if (statusCode === 204 || statusCode === 205 || statusCode === 304) {
        const error = new Error(`Responses with ${statusCode} status must not have a payload`);
        error.code = 'ERR_HTTP2_PAYLOAD_FORBIDDEN';
        throw error;
      }
      if (options.onError !== undefined && typeof options.onError !== 'function') {
        throw invalidFileOption('onError', 'function', options.onError);
      }

      const statTarget = fromFd ? fs.fstat.bind(fs, target) : fs.stat.bind(fs, target);
      statTarget((statError, stat) => {
        if (statError) {
          if (options.onError) options.onError(statError);
          else this.destroy(statError);
          return;
        }
        const statOptions = {
          offset: options.offset === undefined ? 0 : options.offset,
          length: options.length === undefined ? -1 : options.length,
        };
        if (stat.isDirectory()) {
          const error = new Error('Directories cannot be sent');
          error.code = 'ERR_HTTP2_SEND_FILE';
          if (options.onError) options.onError(error);
          else this.destroy(error);
          return;
        }
        if (!stat.isFile() && (options.offset !== undefined || options.length !== undefined)) {
          const error = new Error('Offset or length can only be specified for regular files');
          error.code = 'ERR_HTTP2_SEND_FILE_NOSEEK';
          if (options.onError) options.onError(error);
          else this.destroy(error);
          return;
        }
        if (options.statCheck && options.statCheck.call(this, stat, responseHeaders, statOptions) === false) {
          return;
        }
        read((readError, data) => {
          if (readError) {
            if (options.onError) options.onError(readError);
            else this.destroy(readError);
            return;
          }
          const start = Math.max(0, statOptions.offset);
          const end = statOptions.length < 0 ? data.length : Math.min(data.length, start + statOptions.length);
          const payload = !stat.isFile() ? data : data.subarray(start, end);
          if (responseHeaders[':status'] === undefined) responseHeaders[':status'] = 200;
          if (responseHeaders['content-length'] === undefined) responseHeaders['content-length'] = payload.length;
          if (responseHeaders['last-modified'] === undefined && stat.isFile()) {
            responseHeaders['last-modified'] = stat.mtime.toUTCString();
          }
          if (responseHeaders['accept-ranges'] === undefined && stat.isFile()) {
            responseHeaders['accept-ranges'] = 'bytes';
          }
          this.respond(responseHeaders);
          if (!this.headRequest) this.end(payload);
        });
      });
    }

    close(code = 0) {
      if (typeof code !== 'number') {
        const error = new TypeError(
          `The "code" argument must be of type number. ${receivedArgument(code)}`
        );
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      if (!Number.isInteger(code)) {
        const error = new RangeError(
          `The value of "code" is out of range. It must be an integer. Received ${code}`
        );
        error.code = 'ERR_OUT_OF_RANGE';
        throw error;
      }
      if (code < 0 || code > 0xffffffff) {
        const error = new RangeError(
          `The value of "code" is out of range. It must be >= 0 && <= 4294967295. Received ${code}`
        );
        error.code = 'ERR_OUT_OF_RANGE';
        throw error;
      }
      this.rstCode = code;
      this._h2Closed = true;
      this._sendReset(code);
      if (code && code !== 8) {
        const error = new Error(`Stream closed with error code ${streamErrorCodeName(code)}`);
        error.code = 'ERR_HTTP2_STREAM_ERROR';
        error.errno = code;
        this.destroy(error);
      } else {
        this.destroy();
      }
      return this;
    }

    _sendReset(code) {
      const socket = this.session?.socket;
      if (!socket || socket.destroyed || socket.writableEnded) return;
      binding.submitRstStream(this.session._id, this.id, code >>> 0);
      this.session._flush();
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
        throw new TypeError('The "headers" argument must be of type object.');
      }
      validateResponseHeaderNames(headers, false);
      this._trailersSent = true;
      binding.submitHeaders(this.session._id, this.id,
        Object.entries(headers)
          .filter(([, value]) => validHeaderValue(value))
          .map(([name, value]) => [name.toLowerCase(), String(value)]), true);
      this.session._flush();
      const callback = this._trailersFinalCallback;
      this._trailersFinalCallback = undefined;
      if (callback) callback();
    }

    pushStream(headers = {}, options, callback) {
      if (typeof options === 'function') {
        callback = options;
        options = {};
      }
      if (this._isPushStream) {
        const error = new Error('A pushed stream cannot create another pushed stream');
        error.code = 'ERR_HTTP2_NESTED_PUSH';
        throw error;
      }
      if (!this.pushAllowed) {
        const error = new Error('HTTP/2 client has disabled push streams');
        error.code = 'ERR_HTTP2_PUSH_DISABLED';
        throw error;
      }
      if (typeof callback !== 'function') {
        const error = new TypeError('The "callback" argument must be of type function.');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      if (headers === null || typeof headers !== 'object' || Array.isArray(headers)) {
        throw new TypeError('The "headers" argument must be of type object.');
      }
      for (const name of Object.keys(headers)) {
        const lower = name.toLowerCase();
        if (['connection', 'keep-alive', 'proxy-connection', 'transfer-encoding', 'upgrade'].includes(lower)) {
          const error = new TypeError(`HTTP/1 Connection specific headers are forbidden: "${lower}"`);
          error.code = 'ERR_HTTP2_INVALID_CONNECTION_HEADERS';
          throw error;
        }
      }
      const pushedHeaders = Object.assign(Object.create(null), {
        ':method': 'GET',
        ':scheme': this.headers[':scheme'] || 'https',
        ':authority': this.headers[':authority'],
        ':path': '/',
      }, headers);
      const pairs = Object.entries(pushedHeaders).map(([name, value]) => [
        name.startsWith(':') ? name : name.toLowerCase(), String(value),
      ]);
      const id = binding.submitPushPromise(this.session._id, this.id, pairs);
      const pushed = new ServerHttp2Stream(this.session, id, pushedHeaders,
        pairs.flatMap(([name, value]) => [name, value]));
      pushed._isPushStream = true;
      this.session._streams.set(id, pushed);
      if (pushedHeaders[':method'] === 'HEAD') pushed.end();
      if (streamCreatedChannel.hasSubscribers) {
        streamCreatedChannel.publish({ stream: pushed, headers: pushedHeaders });
      }
      process.nextTick(() => {
        if (streamStartChannel.hasSubscribers) {
          streamStartChannel.publish({ stream: pushed, headers: pushedHeaders });
        }
      });
      pushed.once('error', (error) => {
        if (streamErrorChannel.hasSubscribers) streamErrorChannel.publish({ stream: pushed, error });
      });
      pushed.once('close', () => {
        if (streamCloseChannel.hasSubscribers) streamCloseChannel.publish({ stream: pushed });
      });
      this.session._flush();
      if (typeof callback === 'function') process.nextTick(callback, null, pushed);
      return this;
    }
  }

  class Http2ServerSession extends EventEmitter {
    constructor(socket, owner) {
      super();
      this.socket = socket;
      this[socketSymbol] = socket;
      this.server = owner;
      this.type = 'server';
      this.closed = false;
      this.destroyed = false;
      this._id = binding.newSession(true);
      if (owner._options.connectionWindowSize !== undefined) {
        binding.setLocalWindowSize(this._id, owner._options.connectionWindowSize);
      }
      this._lastStreamID = 0;
      this._originReady = false;
      this._pendingOriginFrames = [];
      this._frameInspectionBuffer = Buffer.alloc(0);
      this._clientPrefaceInspected = false;
      this._streams = new Map();
      this._pendingPings = new Map();
      this.state = {
        effectiveLocalWindowSize: owner._options.connectionWindowSize ?? 65535,
        localWindowSize: Math.max(65535, owner._options.connectionWindowSize ?? 65535),
        remoteWindowSize: 65535,
      };
      this.localSettings = {
        headerTableSize: 4096, enablePush: true, maxConcurrentStreams: 0xffffffff,
        initialWindowSize: 65535, maxFrameSize: 16384, maxHeaderListSize: 65535,
        maxHeaderSize: 65535, enableConnectProtocol: false, customSettings: Object.create(null),
        ...(owner._options.settings || {}),
      };
      this.localSettings.customSettings = {
        ...(owner._options.settings?.customSettings || {}),
      };
      this._initialSettingsFramePending = true;
      this.remoteSettings = { ...this.localSettings, customSettings: Object.create(null) };
      this.remoteCustomSettings = new Set(owner._options.remoteCustomSettings || []);
      this._settingsPendingCount = 1;
      this.pendingSettingsAck = true;
      this._sessionEmitted = false;
      this._finishEmitted = false;
      this._onData = (chunk) => this._receive(chunk);
      socket.on('data', this._onData);
      socket.on('end', () => this._finish());
      socket.on('error', (error) => {
        if (!this.destroyed) this.emit('error', error);
      });
      socket.on('close', () => this._finish());
    }

    _flush() {
      if (this.destroyed || this.socket.destroyed || this.socket.writableEnded) return;
      const outbound = binding.drainOutbound(this._id);
      let data = outbound && Buffer.from(outbound);
      if (data && data.length && this._initialSettingsFramePending) {
        const initialLength = data.readUIntBE(0, 3);
        const initialSettings = data.length >= 9 + initialLength &&
          data[3] === 0x4 && data[4] === 0 && data.readUInt32BE(5) === 0;
        if (initialSettings) {
          this._initialSettingsFramePending = false;
          const configured = this.server._settingsPayload;
          if (configured !== undefined) {
            data = Buffer.concat([
              http2Frame(0x4, 0, configured),
              data.subarray(9 + initialLength),
            ]);
          }
        }
      }
      if (data && data.length && !this.socket.destroyed && !this.socket.writableEnded) this.socket.write(data);
    }

    _emitSession() {
      if (this._sessionEmitted) return;
      this._sessionEmitted = true;
      this.server.emit('session', this);
    }

    _receive(chunk) {
      if (this.destroyed || this._protocolFailed) return;
      this.server._resetSessionTimeout(this);
      if (this._inspectOversizedFrame(chunk)) {
        this._protocolFailed = true;
        this.closed = true;
        const error = new Error('Protocol error');
        error.code = 'ERR_HTTP2_ERROR';
        this.emit('error', error);
        this._flush();
        const payload = Buffer.alloc(8);
        payload.writeUInt32BE(this._lastStreamID & 0x7fffffff, 0);
        payload.writeUInt32BE(6, 4); // NGHTTP2_FRAME_SIZE_ERROR
        if (!this.socket.destroyed && !this.socket.writableEnded) {
          this.socket.write(http2Frame(0x7, 0, payload));
          this.socket.end();
        }
        return;
      }
      if (this.destroyed || this._protocolFailed) return;
      try {
        const events = binding.feed(this._id, chunk);
        this._originReady = true;
        this._emitSession();
        for (const event of events) this._dispatch(event);
        this._flush();
        for (const frame of this._pendingOriginFrames.splice(0)) {
          if (!this.socket.destroyed && !this.socket.writableEnded) this.socket.write(frame);
        }
        if (this._protocolFailed && !this.socket.destroyed && !this.socket.writableEnded) {
          this.socket.end();
        }
      } catch (error) {
        if (error && typeof error === 'object' && !(error instanceof NghttpError)) {
          Object.setPrototypeOf(error, NghttpError.prototype);
          if (error.code === undefined) error.code = 'ERR_HTTP2_ERROR';
        }
        // nghttp2 can queue a GOAWAY before reporting a fatal receive error.
        // Flush it and half-close the socket so peers observe the protocol
        // error and cleanly complete their end/close lifecycle.
        this._protocolFailed = true;
        this.closed = true;
        if (!this.destroyed) this.emit('error', error);
        try { this._flush(); } catch {}
        if (!this.socket.destroyed && !this.socket.writableEnded) this.socket.end();
      }
    }

    _inspectOversizedFrame(chunk) {
      this._frameInspectionBuffer = Buffer.concat([
        this._frameInspectionBuffer,
        Buffer.from(chunk),
      ]);
      if (!this._clientPrefaceInspected) {
        const preface = Buffer.from('PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n');
        const compared = Math.min(preface.length, this._frameInspectionBuffer.length);
        if (!this._frameInspectionBuffer.subarray(0, compared).equals(preface.subarray(0, compared))) {
          this._frameInspectionBuffer = Buffer.alloc(0);
          this._clientPrefaceInspected = true;
          return false;
        }
        if (this._frameInspectionBuffer.length < preface.length) return false;
        this._frameInspectionBuffer = this._frameInspectionBuffer.subarray(preface.length);
        this._clientPrefaceInspected = true;
      }
      while (this._frameInspectionBuffer.length >= 9) {
        const frameLength = this._frameInspectionBuffer.readUIntBE(0, 3);
        if (frameLength > 16384) return true;
        if (this._frameInspectionBuffer.length < 9 + frameLength) return false;
        if (this._frameInspectionBuffer[3] === 0x8 && frameLength === 4 &&
            (this._frameInspectionBuffer.readUInt32BE(5) & 0x7fffffff) === 0) {
          this.state.remoteWindowSize +=
            this._frameInspectionBuffer.readUInt32BE(9) & 0x7fffffff;
        }
        if (this._frameInspectionBuffer[3] === 0x6 && frameLength === 8 &&
            this._frameInspectionBuffer.readUInt32BE(5) === 0) {
          const payload = this._frameInspectionBuffer.subarray(9, 17);
          if (this._frameInspectionBuffer[4] & 1) {
            const key = payload.toString('hex');
            const pending = this._pendingPings.get(key);
            if (pending) {
              this._pendingPings.delete(key);
              if (pending.callback) {
                try {
                  pending.asyncResource.runInAsyncScope(
                    pending.callback, undefined, null, Date.now() - pending.started,
                    Buffer.from(payload),
                  );
                } finally {
                  pending.asyncResource.emitDestroy();
                }
              }
            }
          } else {
            this.emit('ping', Buffer.from(payload));
          }
        }
        if (this._frameInspectionBuffer[3] === 0x4 &&
            (this._frameInspectionBuffer[4] & 1) === 0 && frameLength % 6 === 0) {
          const payload = this._frameInspectionBuffer.subarray(9, 9 + frameLength);
          if (payload.length === 0) {
            if (!Number.isInteger(this.server._options.maxSettings)) {
              this.emit('remoteSettings', this.remoteSettings);
            }
            this._frameInspectionBuffer = this._frameInspectionBuffer.subarray(9 + frameLength);
            continue;
          }
          if (Number.isInteger(this.server._options.maxSettings) &&
              payload.length / 6 > this.server._options.maxSettings) {
            this._protocolFailed = true;
            this.closed = true;
            // A well-formed preface has established an HTTP/2 session even
            // when its first SETTINGS frame violates maxSettings. Node emits
            // the session event before tearing this connection down.
            this._emitSession();
            this.destroy();
            return false;
          }
          const remote = { ...this.remoteSettings, customSettings: Object.create(null) };
          for (let offset = 0; offset < payload.length; offset += 6) {
            const id = payload.readUInt16BE(offset);
            const value = payload.readUInt32BE(offset + 2);
            const name = Object.keys(settingIds).find((key) => settingIds[key] === id && key !== 'maxHeaderSize');
            if (name) remote[name] = id === 2 || id === 8 ? value !== 0 : value;
            else if (id >= 0x10 && this.remoteCustomSettings.has(id)) remote.customSettings[id] = value;
          }
          this.remoteSettings = remote;
          this.emit('remoteSettings', remote);
        }
        if (this._frameInspectionBuffer[3] === 0x4 &&
            (this._frameInspectionBuffer[4] & 1) !== 0) {
          this._settingsPendingCount = Math.max(0, this._settingsPendingCount - 1);
          this.pendingSettingsAck = this._settingsPendingCount > 0;
          this.emit('localSettings', this.localSettings);
        }
        this._frameInspectionBuffer = this._frameInspectionBuffer.subarray(9 + frameLength);
      }
      return false;
    }

    _dispatch(event) {
      if (event.type === 'error') {
        const error = new Error(event.message || 'HTTP/2 session error');
        error.code = event.code;
        this._protocolFailed = true;
        this.closed = true;
        this.emit('error', error);
        return;
      }
      if (event.type === 'headers') {
        const id = event.streamId;
        this._lastStreamID = Math.max(this._lastStreamID, id);
        const existing = this._streams.get(id);
        if (!existing) {
          const settings = this.server._options.settings || {};
          const maxHeaderListSize = settings.maxHeaderListSize ?? settings.maxHeaderSize ?? 65535;
          const maxHeaderListPairs = Math.max(4, this.server._options.maxHeaderListPairs ?? 128);
          if (headerListSize(event.headers) > maxHeaderListSize ||
              (event.headers || []).length > maxHeaderListPairs) {
            binding.submitRstStream(this._id, id, 11); // NGHTTP2_ENHANCE_YOUR_CALM
            return;
          }
        }
        const values = objectHeaders(event.headers || []);
        if (existing) {
          existing.emit('trailers', values.headers, 0, values.rawHeaders);
          if (event.endStream) existing.push(null);
          return;
        }
        const stream = new ServerHttp2Stream(this, id, values.headers, values.rawHeaders);
        this._streams.set(id, stream);
        if (streamCreatedChannel.hasSubscribers) {
          streamCreatedChannel.publish({ stream, headers: values.headers });
        }
        process.nextTick(() => {
          if (streamStartChannel.hasSubscribers) {
            streamStartChannel.publish({ stream, headers: values.headers });
          }
        });
        stream.once('error', (error) => {
          if (streamErrorChannel.hasSubscribers) streamErrorChannel.publish({ stream, error });
        });
        stream.once('close', () => {
          if (streamCloseChannel.hasSubscribers) streamCloseChannel.publish({ stream });
        });
        if (event.endStream) stream.push(null);
        const flags = 4 | (event.endStream ? 1 : 0);
        const compatListeners = ['request', 'connect', 'checkContinue', 'checkExpectation'];
        if (compatListeners.some((name) => this.server.listenerCount(name) > 0)) {
          compat.onServerStream.call(this.server,
            this.server._options.Http2ServerRequest || compat.Http2ServerRequest,
            this.server._options.Http2ServerResponse || compat.Http2ServerResponse,
            stream, values.headers, flags, values.rawHeaders);
        }
        this.server.emit('stream', stream, values.headers, flags, values.rawHeaders);
        return;
      }
      if (event.type === 'data') {
        const stream = this._streams.get(event.streamId);
        if (stream) {
          if (event.data && event.data.length) stream.push(event.data);
          if (event.endStream) stream.push(null);
        }
        return;
      }
      if (event.type === 'close') {
        const stream = this._streams.get(event.streamId);
        if (stream) {
          stream.setTimeout(0);
          stream.rstCode = event.code || 0;
          if (!stream._h2Closed) {
            stream._h2Closed = true;
            if (event.code) {
              const error = new Error(`Stream closed with error code ${streamErrorCodeName(event.code)}`);
              error.code = 'ERR_HTTP2_STREAM_ERROR';
              error.errno = event.code;
              stream.emit('error', error);
            }
            if (!stream.readableEnded) stream.push(null);
            stream.emit('close');
          }
          this._streams.delete(event.streamId);
          this._maybeCloseSocket();
        }
      }
    }

    _finish() {
      if (this._finishEmitted) return;
      this._finishEmitted = true;
      clearTimeout(this._serverTimeoutTimer);
      this._serverTimeoutTimer = undefined;
      this.closed = true;
      if (!this.destroyed) {
        this.destroyed = true;
        binding.closeSession(this._id);
      }
      for (const stream of this._streams.values()) {
        if (!stream._h2Closed) {
          stream._h2Closed = true;
          if (!stream.readableEnded) stream.push(null);
          stream.emit('close');
        }
      }
      this._streams.clear();
      this.emit('close');
    }

    close(callback) {
      if (typeof callback === 'function') this.once('close', callback);
      if (this.closed) return this;
      this.closed = true;
      this._flush();
      if (!this.socket.destroyed && !this.socket.writableEnded) {
        const payload = Buffer.alloc(8);
        payload.writeUInt32BE(this._lastStreamID & 0x7fffffff, 0);
        payload.writeUInt32BE(0, 4);
        this.socket.write(http2Frame(0x7, 0, payload));
      }
      this._maybeCloseSocket();
      return this;
    }

    _maybeCloseSocket() {
      if (this.closed && this._streams.size === 0 &&
          !this.socket.destroyed && !this.socket.writableEnded) {
        this.socket.end();
      }
    }

    goaway(code = 0, lastStreamID = 0, opaqueData) {
      if (this.destroyed) {
        const error = new Error('The session has been destroyed');
        error.code = 'ERR_HTTP2_INVALID_SESSION';
        throw error;
      }
      if (typeof code !== 'number' || !Number.isFinite(code)) {
        const error = new TypeError(
          `The "code" argument must be of type number. ${receivedArgument(code)}`,
        );
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      if (typeof lastStreamID !== 'number' || !Number.isFinite(lastStreamID)) {
        const error = new TypeError(
          `The "lastStreamID" argument must be of type number. ${receivedArgument(lastStreamID)}`,
        );
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      if (!Number.isInteger(code) || code < 0 || code > 0xffffffff) {
        const error = new RangeError('The value of "code" is out of range.');
        error.code = 'ERR_OUT_OF_RANGE';
        throw error;
      }
      if (!Number.isInteger(lastStreamID) || lastStreamID < 0 || lastStreamID > 0x7fffffff) {
        const error = new RangeError('The value of "lastStreamID" is out of range.');
        error.code = 'ERR_OUT_OF_RANGE';
        throw error;
      }
      if (opaqueData !== undefined && !Buffer.isBuffer(opaqueData)) {
        if (ArrayBuffer.isView(opaqueData)) {
          opaqueData = Buffer.from(
            opaqueData.buffer, opaqueData.byteOffset, opaqueData.byteLength,
          );
        } else {
          const error = new TypeError(
            `The "opaqueData" argument must be an instance of Buffer, TypedArray, or DataView. ${receivedArgument(opaqueData)}`,
          );
          error.code = 'ERR_INVALID_ARG_TYPE';
          throw error;
        }
      }
      const opaque = opaqueData || Buffer.alloc(0);
      const payload = Buffer.allocUnsafe(8 + opaque.length);
      // libnghttp2 treats zero as the highest stream seen when emitting a
      // GOAWAY frame, which is also the behavior callers expect from the
      // default lastStreamID.
      payload.writeUInt32BE(lastStreamID || this._lastStreamID, 0);
      payload.writeUInt32BE(code, 4);
      opaque.copy(payload, 8);
      this._flush();
      if (!this.socket.destroyed) this.socket.write(http2Frame(0x7, 0, payload));
    }

    altsvc(alt, originOrStream) {
      if (this.destroyed) {
        const error = new Error('The session has been destroyed');
        error.code = 'ERR_HTTP2_INVALID_SESSION';
        throw error;
      }
      let streamId = 0;
      let origin = '';
      let hasOrigin = false;
      if (typeof originOrStream === 'number') {
        if (!Number.isInteger(originOrStream) || originOrStream <= 0 || originOrStream >= 0x100000000) {
          const error = new RangeError(`The value of "originOrStream" is out of range. It must be > 0 && < 4294967296. Received ${originOrStream}`);
          error.code = 'ERR_OUT_OF_RANGE';
          throw error;
        }
        streamId = originOrStream;
      } else if (typeof originOrStream === 'string') {
        hasOrigin = true;
        try {
          origin = new URL(originOrStream).origin;
        } catch {
          origin = 'null';
        }
      } else if (originOrStream !== undefined && originOrStream !== null && typeof originOrStream === 'object') {
        hasOrigin = true;
        origin = originOrStream.origin;
        if (typeof origin !== 'string') {
          const error = new TypeError('The "originOrStream" argument must be of type string, number, URL, or object.');
          error.code = 'ERR_INVALID_ARG_TYPE';
          throw error;
        }
      } else if (originOrStream !== undefined) {
        const error = new TypeError('The "originOrStream" argument must be of type string, number, URL, or object.');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      if (typeof alt !== 'string') {
        const error = new TypeError('The "alt" argument must be of type string.');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      if (!/^[\x20-\x7e]*$/.test(alt)) {
        const error = new TypeError('Invalid character in alt');
        error.code = 'ERR_INVALID_CHAR';
        throw error;
      }
      if (hasOrigin && (typeof origin !== 'string' || origin === '' || origin === 'null')) {
        const error = new TypeError('HTTP/2 ALTSVC frames require a valid origin');
        error.code = 'ERR_HTTP2_ALTSVC_INVALID_ORIGIN';
        throw error;
      }
      if ((alt.length + origin.length) > 16382) {
        const error = new TypeError('HTTP/2 ALTSVC frames are limited to 16382 bytes');
        error.code = 'ERR_HTTP2_ALTSVC_LENGTH';
        throw error;
      }
      if (streamId && !this._streams.has(streamId)) return;
      const altBytes = Buffer.from(alt);
      const originBytes = Buffer.from(origin);
      const prefix = streamId ? 0 : 2 + originBytes.length;
      const payload = Buffer.allocUnsafe(prefix + altBytes.length);
      if (!streamId) {
        payload.writeUInt16BE(originBytes.length, 0);
        originBytes.copy(payload, 2);
      }
      altBytes.copy(payload, prefix);
      this._flush();
      if (!this.socket.destroyed && !this.socket.writableEnded) {
        this.socket.write(http2Frame(0x0a, streamId, payload));
      }
    }

    origin(...origins) {
      if (this.destroyed) {
        const error = new Error('The session has been destroyed');
        error.code = 'ERR_HTTP2_INVALID_SESSION';
        throw error;
      }
      if (origins.length === 0) return;
      const values = origins.map((value) => {
        let origin;
        if (typeof value === 'string') {
          try {
            origin = new URL(value).origin;
          } catch (error) {
            throw error;
          }
        } else if (value !== null && typeof value === 'object') {
          origin = value.origin;
        }
        if (typeof origin !== 'string') {
          const error = new TypeError('The "origin" argument must be of type string.');
          error.code = 'ERR_INVALID_ARG_TYPE';
          throw error;
        }
        if (origin === 'null') {
          const error = new TypeError('The origin must be a valid URL origin');
          error.code = 'ERR_HTTP2_INVALID_ORIGIN';
          throw error;
        }
        return Buffer.from(origin);
      });
      const length = values.reduce((sum, value) => sum + value.length, 0);
      if (length > 16384) {
        const error = new TypeError('The origin list is too long');
        error.code = 'ERR_HTTP2_ORIGIN_LENGTH';
        throw error;
      }
      const payload = Buffer.allocUnsafe(values.reduce((sum, value) => sum + 2 + value.length, 0));
      let offset = 0;
      for (const value of values) {
        payload.writeUInt16BE(value.length, offset);
        offset += 2;
        value.copy(payload, offset);
        offset += value.length;
      }
      const frame = http2Frame(0x0c, 0, payload);
      if (!this._originReady || this._pendingOriginFrames.length) this._pendingOriginFrames.push(frame);
      else {
        this._flush();
        if (!this.socket.destroyed && !this.socket.writableEnded) this.socket.write(frame);
      }
    }

    destroy(error) {
      if (this.destroyed) return this;
      clearTimeout(this._serverTimeoutTimer);
      this._serverTimeoutTimer = undefined;
      this.destroyed = true;
      this.closed = true;
      for (const pending of this._pendingPings.values()) {
        if (pending.callback) {
          const pingError = new Error('HTTP/2 ping cancelled');
          pingError.code = 'ERR_HTTP2_PING_CANCEL';
          try { pending.asyncResource.runInAsyncScope(pending.callback, undefined, pingError); }
          finally { pending.asyncResource.emitDestroy(); }
        }
      }
      this._pendingPings.clear();
      binding.closeSession(this._id);
      if (error) this.emit('error', error);
      this.socket.destroy();
      return this;
    }

    ref() { if (typeof this.socket.ref === 'function') this.socket.ref(); return this; }
    unref() { if (typeof this.socket.unref === 'function') this.socket.unref(); return this; }
    setTimeout(timeout, callback) { this.socket.setTimeout(timeout, callback); return this; }

    ping(payload, callback) {
      if (typeof payload === 'function') {
        callback = payload;
        payload = undefined;
      }
      if (payload === undefined) payload = Buffer.alloc(8);
      if (!Buffer.isBuffer(payload) || payload.length !== 8) {
        const error = new TypeError('The "payload" argument must be an 8-byte Buffer.');
        error.code = 'ERR_HTTP2_PING_LENGTH';
        throw error;
      }
      if (callback !== undefined && typeof callback !== 'function') {
        const error = new TypeError('The "callback" argument must be of type function.');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      if (this.destroyed || this.closed) {
        const error = new Error('HTTP/2 ping cancelled');
        error.code = 'ERR_HTTP2_PING_CANCEL';
        if (callback) process.nextTick(callback, error);
        return false;
      }
      const asyncResource = callback
        ? new (process.getBuiltinModule('async_hooks').AsyncResource)('HTTP2PING')
        : undefined;
      if (this._pendingPings.size >= (this.server._options.maxOutstandingPings || 10)) {
        if (callback) {
          const error = new Error('HTTP2 ping cancelled');
          error.code = 'ERR_HTTP2_PING_CANCEL';
          process.nextTick(() => {
            try { asyncResource.runInAsyncScope(callback, undefined, error); }
            finally { asyncResource.emitDestroy(); }
          });
        }
        return false;
      }
      const key = payload.toString('hex');
      this._pendingPings.set(key, {
        callback, asyncResource, started: Date.now(), payload: Buffer.from(payload),
      });
      this._flush();
      this.socket.write(http2Frame(0x6, 0, payload));
      return true;
    }

    settings(settings, callback) {
      if (callback !== undefined && typeof callback !== 'function') {
        const error = new TypeError('The "callback" argument must be of type function.');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      if (settings === undefined) settings = {};
      const { pairs } = encodeSettings(settings);
      if (callback) this.once('localSettings', callback);
      this._settingsPendingCount++;
      this.pendingSettingsAck = true;
      if (this._settingsPendingCount > (this.server._options.maxOutstandingSettings || 10)) {
        const error = new Error('Maximum number of outstanding settings frames');
        error.code = 'ERR_HTTP2_MAX_PENDING_SETTINGS_ACK';
        this.emit('error', error);
        this.destroyed = true;
        this.socket.destroy();
        return undefined;
      }
      binding.submitSettings(this._id, pairs);
      this._flush();
      this.localSettings = { ...this.localSettings, ...settings };
      this.localSettings.customSettings = { ...(settings.customSettings || this.localSettings.customSettings) };
      return undefined;
    }

    setLocalWindowSize(windowSize) {
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
      const previous = this.state.localWindowSize;
      this.state.effectiveLocalWindowSize = windowSize;
      if (windowSize > previous) {
        this.state.localWindowSize = windowSize;
      }
    }
  }

  class Http2Server extends EventEmitter {
    emit(event, ...args) {
      // Node's net.Server turns a manually emitted connection event into a
      // new connection. Preserve that behavior for the wrapper server too,
      // which is used by tests and embedders with generic Duplex streams.
      if (event === 'connection' && args[0] !== undefined) {
        this._accept(args[0]);
        return this.listenerCount('connection') > 0;
      }
      return super.emit(event, ...args);
    }

    constructor(options, requestListener, secure) {
      super();
      this._secure = secure;
      this._options = options || {};
      validateConnectionWindowSize(this._options.connectionWindowSize);
      this._settingsPayload = this._options.settings === undefined
        ? undefined
        : encodeSettings(this._options.settings).payload;
      this[Symbol('options')] = this._options;
      if (secure && this._options.ALPNCallback && this._options.ALPNProtocols) {
        const error = new TypeError('The ALPNCallback and ALPNProtocols TLS options are mutually exclusive');
        error.code = 'ERR_TLS_ALPN_CALLBACK_WITH_PROTOCOLS';
        throw error;
      }
      this.allowHalfOpen = this._options.allowHalfOpen === true;
      this._allowHTTP1 = secure && this._options.allowHTTP1 === true;
      this._http1Server = this._allowHTTP1
        ? http.createServer(this._options.http1Options || {})
        : undefined;
      if (this._http1Server) {
        for (const name of ['request', 'upgrade', 'connect', 'checkContinue', 'checkExpectation']) {
          this._http1Server.on(name, (...args) => this.emit(name, ...args));
        }
        this._http1Server.on('clientError', (error, socket) => {
          if (this.listenerCount('clientError')) this.emit('clientError', error, socket);
          else socket.destroy(error);
        });
      }
      this._timeout = 0;
      this._timeoutCallback = undefined;
      this._requestListener = requestListener;
      this._sessions = new Set();
      this._http1Sockets = new Set();
      let server;
      if (secure) {
        const tlsOptions = { ...this._options };
        delete tlsOptions.settings;
        delete tlsOptions.allowHTTP1;
        if (tlsOptions.ALPNProtocols === undefined &&
            tlsOptions.ALPNCallback === undefined) {
          // Keep negotiation on the TLS server's supported ALPNProtocols path.
          // The TLS host does not implement Node's ALPNCallback contract.
          tlsOptions.ALPNProtocols = this._allowHTTP1 ? ['h2', 'http/1.1'] : ['h2'];
        }
        server = tls.createServer(tlsOptions);
        server.on('secureConnection', (socket) => {
          socket.allowHalfOpen = this.allowHalfOpen;
          if (this._allowHTTP1) {
            this.emit('secureConnection', socket);
            this._routeSecureSocket(socket, true);
          } else if (socket.alpnProtocol === 'h2') {
            this._accept(socket);
          } else {
            this._unknownProtocol(socket);
          }
        });
      } else {
        server = net.createServer({ allowHalfOpen: this.allowHalfOpen }, (socket) => this._accept(socket));
      }
      this._server = server;
      for (const name of ['listening', 'close', 'error']) {
        server.on(name, (...args) => this.emit(name, ...args));
      }
      if (typeof requestListener === 'function') this.on('request', requestListener);
    }

    _routeSecureSocket(socket, secureConnectionEmitted = false) {
      if (socket.alpnProtocol !== 'h2') {
        this._acceptHttp1(socket);
        return;
      }

      const preface = Buffer.from('PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n');
      let received = Buffer.alloc(0);
      const onData = (chunk) => {
        received = Buffer.concat([received, Buffer.from(chunk)]);
        const compared = Math.min(received.length, preface.length);
        let matchesPreface = true;
        for (let index = 0; index < compared; index++) {
          if (received[index] !== preface[index]) {
            matchesPreface = false;
            break;
          }
        }
        if (matchesPreface && received.length < preface.length) return;
        socket.removeListener('data', onData);
        if (matchesPreface) this._accept(socket, received, secureConnectionEmitted);
        else this._acceptHttp1(socket, received);
      };
      socket.on('data', onData);
    }

    _unknownProtocol(socket) {
      const handled = this.emit('unknownProtocol', socket);
      if (handled || socket.destroyed) return;
      socket.end('HTTP/1.0 403 Forbidden\r\n' +
        'Content-Type: text/plain\r\n\r\n' +
        'Missing ALPN Protocol, expected `h2` to be available.\n' +
        'If this is a HTTP request: The server was not configured with the `allowHTTP1` option or a listener for the `unknownProtocol` event.\n');
    }

    _acceptHttp1(socket, initialData) {
      if (!this._http1Sockets.has(socket)) {
        this._http1Sockets.add(socket);
        socket.once('close', () => this._http1Sockets.delete(socket));
      }
      let received = initialData ? Buffer.from(initialData) : Buffer.alloc(0);
      let handled = false;
      const onData = (chunk) => {
        if (handled) return;
        if (chunk.length) received = Buffer.concat([received, Buffer.from(chunk)]);
        const headEnd = received.indexOf('\r\n\r\n');
        if (headEnd < 0) return;
        handled = true;
        socket.removeListener('data', onData);

        const head = received.subarray(0, headEnd).toString('latin1');
        const lines = head.split('\r\n');
        const [method = '', url = '', protocolVersion = 'HTTP/1.1'] = lines.shift().split(' ');
        const version = protocolVersion.startsWith('HTTP/')
          ? protocolVersion.slice('HTTP/'.length)
          : protocolVersion;
        const headers = Object.create(null);
        const rawHeaders = [];
        for (const line of lines) {
          const colon = line.indexOf(':');
          if (colon <= 0) continue;
          const rawName = line.slice(0, colon).trim();
          const name = rawName.toLowerCase();
          const value = line.slice(colon + 1).trim();
          rawHeaders.push(rawName, value);
          if (Object.hasOwn(headers, name)) {
            headers[name] = name === 'set-cookie'
              ? [...(Array.isArray(headers[name]) ? headers[name] : [headers[name]]), value]
              : `${headers[name]}, ${value}`;
          } else {
            headers[name] = value;
          }
        }
        const IncomingMessage = this._http1Server._quenchIncomingMessage || EventEmitter;
        let req;
        try {
          req = new IncomingMessage(socket);
        } catch {
          req = new EventEmitter();
        }
        Object.assign(req, {
          headers,
          rawHeaders,
          trailers: Object.create(null),
          rawTrailers: [],
          method,
          url,
          httpVersionMajor: 1,
          httpVersionMinor: Number(version.split('.')[1] || 1),
          httpVersion: version,
          complete: true,
          aborted: false,
          upgrade: headers.upgrade || null,
          socket,
          connection: socket,
        });
        socket.server = this._http1Server;

        const ServerResponse = this._http1Server._quenchServerResponse;
        let response;
        try {
          response = ServerResponse ? new ServerResponse(req) : {};
        } catch {
          response = {};
        }
        Object.assign(response, {
          statusCode: 200,
          statusMessage: undefined,
          headers: new Map(),
          body: [],
          writeHead(code, reasonOrHeaders, maybeHeaders) {
            this.statusCode = code;
            if (typeof reasonOrHeaders === 'string') {
              this.statusMessage = reasonOrHeaders;
              for (const [name, value] of Object.entries(maybeHeaders || {})) this.setHeader(name, value);
            } else {
              for (const [name, value] of Object.entries(reasonOrHeaders || {})) this.setHeader(name, value);
            }
            return this;
          },
          setHeader(name, value) { this.headers.set(String(name).toLowerCase(), [String(name), value]); return this; },
          getHeader(name) { return this.headers.get(String(name).toLowerCase())?.[1]; },
          write(chunk, encoding) {
            if (chunk !== undefined && chunk !== null) {
              this.body.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(String(chunk), encoding));
            }
            return true;
          },
          end(chunk, encoding, callback) {
            if (typeof encoding === 'function') { callback = encoding; encoding = undefined; }
            if (chunk !== undefined && chunk !== null) this.write(chunk, encoding);
            const body = Buffer.concat(this.body);
            const statusMessage = this.statusMessage || http.STATUS_CODES[this.statusCode] || 'unknown';
            if (!this.headers.has('content-length') && !this.headers.has('transfer-encoding')) {
              this.setHeader('Content-Length', body.length);
            }
            if (!this.headers.has('connection')) this.setHeader('Connection', 'close');
            const headerLines = [...this.headers.values()].map(([name, value]) =>
              `${name}: ${Array.isArray(value) ? value.join(', ') : String(value)}`);
            socket.write(`HTTP/1.1 ${this.statusCode} ${statusMessage}\r\n${headerLines.join('\r\n')}\r\n\r\n`);
            if (body.length) socket.write(body);
            socket.end();
            if (callback) process.nextTick(callback);
            return this;
          },
        });

        const bodyOffset = headEnd + 4;
        const body = received.subarray(bodyOffset);
        if (headers.upgrade && String(headers.connection || '').toLowerCase().split(',').some((part) => part.trim() === 'upgrade')) {
          this._http1Server.emit('upgrade', req, socket, body);
        } else {
          this._http1Server.emit('request', req, response);
        }
      };
      socket.on('data', onData);
      onData(Buffer.alloc(0));
    }

    _accept(socket, initialData, secureConnectionEmitted = false) {
      const session = new Http2ServerSession(socket, this);
      this._sessions.add(session);
      session.once('close', () => this._sessions.delete(session));
      // Emit session before consuming bytes so applications can attach an
      // error listener before malformed client prefaces are reported.
      session._sessionEmitted = true;
      this.emit('session', session);
      session._flush();
      super.emit('connection', socket);
      if (this._secure && !secureConnectionEmitted) this.emit('secureConnection', socket);
      if (this._secure && Array.isArray(this._options.origins) && this._options.origins.length) {
        session.origin(...this._options.origins);
      }
      if (this._timeout > 0) this._applySocketTimeout(session);
      if (initialData?.length) session._receive(initialData);
    }

    _applySocketTimeout(session) {
      // Socket timeout delivery is not reliable for all socket implementations
      // hosted by Quench. Keep the idle timer on the HTTP/2 session and reset
      // it whenever inbound traffic arrives.
      session.socket.setTimeout(0);
      this._resetSessionTimeout(session);
    }

    _resetSessionTimeout(session) {
      clearTimeout(session._serverTimeoutTimer);
      session._serverTimeoutTimer = undefined;
      if (this._timeout <= 0 || session.destroyed || session.closed) return;
      // net.Server accepts a BigInt timeout and retains its original type in
      // `server.timeout`; timer scheduling itself requires a Number delay.
      const timeout = Number(this._timeout);
      session._serverTimeoutTimer = setTimeout(() => {
        session._serverTimeoutTimer = undefined;
        if (session.destroyed || session.closed || this._timeout <= 0) return;
        const handled = this.emit('timeout', session);
        const sessionHandled = session.emit('timeout');
        const callback = this._timeoutCallback;
        if (callback) callback.call(this, session);
        // Match net.Server's timeout behavior: listeners and callbacks own
        // timed-out sockets; otherwise close the idle session by default.
        if (!handled && !sessionHandled && !callback &&
            this.listenerCount('timeout') === 0 && session.listenerCount('timeout') === 0) {
          session.close();
        }
      }, timeout);
    }

    listen(...args) { this._server.listen(...args); return this; }
    address() { return this._server.address(); }

    updateSettings(settings) {
      if (settings === null || typeof settings !== 'object' || Array.isArray(settings)) {
        const received = settings === null ? 'Received null' : `Received type ${typeof settings} ('${String(settings)}')`;
        const error = new TypeError(`The "settings" argument must be of type object. ${received}`);
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      for (const name of Object.keys(settingIds)) {
        if (!Object.hasOwn(settings, name) || name === 'maxHeaderSize') continue;
        let value = settings[name];
        if (typeof value === 'boolean' && (name === 'enablePush' || name === 'enableConnectProtocol')) value = Number(value);
        if (typeof value !== 'number' || !Number.isInteger(value) || value < 0 || value > 0xffffffff ||
            name === 'initialWindowSize' && value > 0x7fffffff ||
            name === 'maxFrameSize' && (value < 16384 || value > 0xffffff) ||
            (name === 'enablePush' || name === 'enableConnectProtocol') && value > 1) {
          const error = new RangeError(`Invalid value for setting "${name}": ${settings[name]}`);
          error.code = 'ERR_HTTP2_INVALID_SETTING_VALUE';
          throw error;
        }
      }
      this._options.settings = { ...(this._options.settings || {}), ...settings };
    }
    get listening() { return this._server.listening; }
    get maxHeadersCount() { return this._server.maxHeadersCount; }
    set maxHeadersCount(value) { this._server.maxHeadersCount = value; }

    setTimeout(timeout, callback) {
      if (callback !== undefined && typeof callback !== 'function') {
        const error = new TypeError('The "callback" argument must be of type function.');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      this._timeout = timeout;
      if (callback !== undefined) this._timeoutCallback = callback;
      for (const session of this._sessions) this._applySocketTimeout(session);
      return this;
    }

    get timeout() { return this._timeout; }
    set timeout(value) { this.setTimeout(value); }

    close(callback) {
      // Node closes idle HTTP/1 keep-alive sockets and gracefully closes all
      // active HTTP/2 sessions when the server is closed.
      for (const socket of this._http1Sockets) socket.destroy();
      for (const session of this._sessions) session.close();
      if (typeof callback === 'function') this._server.close(callback);
      else this._server.close();
      return this;
    }

    ref() { this._server.ref(); return this; }
    unref() { this._server.unref(); return this; }
    getConnections(callback) { return this._server.getConnections(callback); }
  }

  if (typeof Symbol.asyncDispose === 'symbol') {
    Object.defineProperty(Http2Server.prototype, Symbol.asyncDispose, {
      configurable: true,
      value() {
        return new Promise((resolve, reject) => {
          this.close((error) => error ? reject(error) : resolve());
        });
      },
    });
  }

  function normalizeOptions(options, handler, allowHandler) {
    if (typeof options === 'function' && allowHandler) {
      handler = options;
      options = undefined;
    }
    if (options === undefined) options = {};
    if (options === null || typeof options !== 'object' || Array.isArray(options)) {
      const error = new TypeError(
        `The "options" argument must be of type object. ${receivedArgument(options)}`
      );
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (handler !== undefined && typeof handler !== 'function') {
      const error = new TypeError('The "requestListener" argument must be of type function.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    const settings = options.settings;
    if (settings !== undefined &&
        (settings === null || typeof settings !== 'object' || Array.isArray(settings))) {
      const error = new TypeError(
        `The "options.settings" property must be of type object. ${receivedArgument(settings)}`
      );
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    for (const name of ['maxSessionInvalidFrames', 'maxSessionRejectedStreams']) {
      const value = options[name];
      if (value !== undefined && typeof value !== 'number') {
        const error = new TypeError(
          `The "options.${name}" property must be of type number. ${receivedArgument(value)}`
        );
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      if (value !== undefined && (!Number.isInteger(value) || value < 0 || value > 0xffffffff)) {
        const error = new RangeError(
          `The value of "options.${name}" is out of range. It must be >= 0 && <= 4294967295. Received ${String(value)}`
        );
        error.code = 'ERR_OUT_OF_RANGE';
        throw error;
      }
    }
    return [options, handler];
  }

  function createServer(options, handler) {
    [options, handler] = normalizeOptions(options, handler, true);
    return new Http2Server(options, handler, false);
  }

  function createSecureServer(options, handler) {
    [options, handler] = normalizeOptions(options, handler, false);
    return new Http2Server(options, handler, true);
  }

  return {
    createServer,
    createSecureServer,
    Http2Server,
    Http2ServerSession,
    Http2ServerStream: ServerHttp2Stream,
  };
})
