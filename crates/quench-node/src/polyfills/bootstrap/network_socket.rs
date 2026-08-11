//! Polyfill: `network-socket`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchNetSocket = class Socket extends globalThis.__nodeEventEmitter {
  constructor(options = {}) {
    super();
    this.readable = true;
    this.writable = true;
    this.readyState = "open";
    this.allowHalfOpen = false;
    this.destroyed = false;
    this._bufferSize = 0;
    this.bytesRead = 0;
    this.bytesWritten = 0;
    this._handle = options?.handle || null;
    this._boundPort = this._handle?._port;
    this._boundHost = this._handle?._host;
    if (this._handle?.constructor?.name === "BoundSocket") {
      this._handle._assertOpen();
      this._handle._adopted = true;
    }
    this._noDelay = false;
    this._nativeId = 0;
    this._nativeConnected = false;
    this.connecting = false;
    this._nativeEnded = false;
    this._readableEnded = false;
    this._endPending = false;
    this._localEnded = false;
    this._corked = 0;
    this._timeoutTimer = null;
    this._peer = null;
    this._paused = false;
    this._pendingData = [];
    this._pendingWrites = [];
    this.localAddress = undefined;
    this.localPort = 0;
    this.remoteAddress = undefined;
    this.remotePort = 0;
    this._keepAlive = false;
    this._keepAliveDelay = 0;
    this._typeOfService = 0;
    this._refed = true;
  }
  get bufferSize() {
    return this._bufferSize;
  }
  setEncoding(encoding) {
    this.encoding = String(encoding);
    return this;
  }
  resume() {
    if (this.destroyed) return this;
    this._paused = false;
    const pending = this._pendingData;
    this._pendingData = [];
    for (const chunk of pending) {
      if (this.destroyed) break;
      this.emit("data", chunk);
    }
    return this;
  }
  [Symbol.asyncIterator]() {
    const socket = this;
    let waiting;
    let closed = false;
    const cleanup = () => {
      socket.off("data", onData);
      socket.off("end", onEnd);
      socket.off("close", onEnd);
    };
    const onData = (chunk) => {
      if (waiting) {
        const resolve = waiting;
        waiting = undefined;
        resolve({ value: chunk, done: false });
      }
    };
    const onEnd = () => {
      closed = true;
      if (waiting) {
        const resolve = waiting;
        waiting = undefined;
        cleanup();
        resolve({ value: undefined, done: true });
      }
    };
    socket.on("data", onData);
    socket.once("end", onEnd);
    socket.once("close", onEnd);
    return {
      next() {
        const pending = socket._pendingData.shift();
        if (pending) return Promise.resolve({ value: pending, done: false });
        if (closed || socket._readableEnded) {
          cleanup();
          return Promise.resolve({ value: undefined, done: true });
        }
        return new Promise((resolve) => {
          waiting = resolve;
        });
      },
      return() {
        cleanup();
        closed = true;
        return Promise.resolve({ value: undefined, done: true });
      },
      [Symbol.asyncIterator]() {
        return this;
      }
    };
  }
  pause() {
    this._paused = true;
    return this;
  }
  pipe(destination, options = {}) {
    this.on("data", (chunk) => {
      if (!destination.destroyed) destination.write(chunk);
    });
    this.once("end", () => {
      if (options.end !== false && !destination.writableEnded) {
        destination.end();
      }
    });
    return destination;
  }
  setNoDelay(enable = true) {
    const value = Boolean(enable);
    if (value !== this._noDelay) {
      this._noDelay = value;
      if (typeof this._handle?.setNoDelay === "function") {
        this._handle.setNoDelay(value);
      }
    }
    return this;
  }
  setKeepAlive(enable = false, initialDelay = 0) {
    const value = Boolean(enable);
    const delay = Math.max(0, Math.floor((Number(initialDelay) || 0) / 1000));
    if (value === this._keepAlive && delay === this._keepAliveDelay) {
      return this;
    }
    this._keepAlive = value;
    this._keepAliveDelay = delay;
    if (typeof this._handle?.setKeepAlive === "function") {
      this._handle.setKeepAlive(value, delay);
    }
    return this;
  }
  setTimeout(timeout, callback) {
    if (typeof timeout !== "number") {
      throw Object.assign(new TypeError('The "timeout" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
    }
    if (!Number.isFinite(timeout) || timeout < 0) {
      throw Object.assign(new RangeError('The value of "timeout" is out of range'), { code: "ERR_OUT_OF_RANGE" });
    }
    if (callback !== undefined && typeof callback !== "function") {
      throw Object.assign(new TypeError('The "callback" argument must be of type function'), { code: "ERR_INVALID_ARG_TYPE" });
    }
    if (this._timeoutTimer) {
      globalThis.clearTimeout(this._timeoutTimer);
      this._timeoutTimer = null;
    }
    const delay = timeout;
    this.timeout = delay;
    if (delay > 0) {
      if (typeof callback === "function") this.once("timeout", callback);
      this._timeoutTimer = globalThis.setTimeout(() => {
        this._timeoutTimer = null;
        if (!this.destroyed) this.emit("timeout");
      }, delay);
    }
    return this;
  }
  destroy() {
    if (this.destroyed) return this;
    const peer = this._peer;
    this.destroyed = true;
    this.readyState = "closed";
    if (this._timeoutTimer) {
      globalThis.clearTimeout(this._timeoutTimer);
      this._timeoutTimer = null;
    }
    if (this._nativeId) {
      __quench_tcp_close(this._nativeId);
      this._nativeId = 0;
      __quenchNativeSockets.delete(this);
    }
    if (peer && !peer.destroyed && !peer.writable) {
      const error = new Error("read ECONNRESET");
      error.code = "ECONNRESET";
      if (peer.listenerCount("error") > 0) {
        queueMicrotask(() => peer.emit("error", error));
      }
    }
    if (peer && !peer.destroyed) peer.destroy();
    queueMicrotask(() => this.emit("close"));
    return this;
  }
  resetAndDestroy() {
    return this.destroy();
  }
  connect(_options, callback) {
    if (Array.isArray(_options)) {
      if (!_options[globalThis.__quenchNetNormalizedArgsSymbol]) {
        throw Object.assign(new TypeError("The port or options argument must be specified"), { code: "ERR_MISSING_ARGS" });
      }
      callback = _options[1];
      _options = _options[0];
    }
    if (typeof _options !== "object" || _options === null) {
      _options = { port: _options };
    }
    if (this.destroyed) {
      this.destroyed = false;
      this.readable = true;
      this.writable = true;
      this.readyState = "open";
      this._nativeEnded = false;
      this._readableEnded = false;
      this._localEnded = false;
      this.__finishEmitted = false;
      this._peer = null;
      this._pendingData = [];
      this._pendingWrites = [];
      this.bytesRead = 0;
      this.bytesWritten = 0;
    }
    globalThis.__quenchValidateConnectionOptions(_options);
    if (_options.allowHalfOpen !== undefined) {
      this.allowHalfOpen = _options.allowHalfOpen === true;
    }
    this._resolvedAddress = _options._resolvedAddress
      ? _options.host
      : undefined;
    if (
      this._handle?.constructor?.name === "BoundSocket" &&
      (_options.localAddress !== undefined || _options.localPort !== undefined)
    ) {
      throw Object.assign(new TypeError("localAddress and localPort cannot be used with a bound socket"), { code: "ERR_INVALID_ARG_VALUE" });
    }
    const localPort = Number(_options.localPort || 0);
    if (
      localPort &&
      [...__quenchNetServers].some(
        (server) => server.listening && server.address().port === localPort
      )
    ) {
      const error = new Error("address already in use");
      error.code = "EADDRINUSE";
      error.syscall = "connect";
      queueMicrotask(() => this.emit("error", error));
      return this;
    }
    if (!this._handle) this._handle = { setKeepAlive: () => {} };
    if (_options.keepAlive !== undefined) {
      this.setKeepAlive(_options.keepAlive, _options.keepAliveInitialDelay);
    }
    const blockList = _options.blockList;
    const blockAddress =
      _options.host === "localhost"
        ? "127.0.0.1"
        : _options.host || "127.0.0.1";
    if (blockList?.check?.(blockAddress)) {
      const error = new Error(`Cannot connect to ${blockAddress}`);
      error.code = "ERR_IP_BLOCKED";
      queueMicrotask(() => this.emit("error", error));
      return this;
    }
    this.connecting = true;
    if (this._boundPort) {
      this.localPort = this._boundPort;
      this.localAddress = this._boundHost;
    }
    if (typeof callback === "function") this.once("connect", callback);
    if (__quenchNativeTransportRequested(_options)) {
      const nativeHost = _options.host || "127.0.0.1";
      const nativePort = Number(_options.port);
      this._nativeId = __quench_tcp_connect(nativeHost, nativePort);
      this._nativeConnected = true;
      this.localAddress = "127.0.0.1";
      this.localPort = __quench_tcp_local_port(this._nativeId);
      this.remoteAddress = nativeHost;
      this.remotePort = nativePort;
      __quenchNativeSockets.add(this);
    }
    queueMicrotask(() => {
      queueMicrotask(() => {
        this.connecting = false;
        this.emit("connect");
        this.emit("ready");
        queueMicrotask(() => {
          if (this._endPending && !this.destroyed) {
            this._endPending = false;
            this.end();
          }
        });
      });
    });
    if (!__quenchNativeTransportRequested(_options)) {
      queueMicrotask(() => {
        const requestedPort = Number(_options.port || 0);
        const requestedPath = _options.path;
        const server = [...__quenchNetServers].find(
          (candidate) =>
            candidate.listening &&
            (!candidate._ipv6Only || isIPv6(_options.host)) &&
            (!this._resolvedAddress ||
              candidate.address().address === this._resolvedAddress) &&
            ((!requestedPath &&
              (!requestedPort || candidate.address().port === requestedPort)) ||
              (requestedPath && candidate._path === requestedPath))
        );
        const httpServer = [
          ...(globalThis.__quenchHttpServers?.values() || [])
        ].find((candidate) => candidate.listening);
        if (!server && httpServer) {
          const serverSocket = new __quenchNetModule.Socket();
          serverSocket._handle = { setKeepAlive: () => {} };
          serverSocket.allowHalfOpen = httpServer.httpAllowHalfOpen === true;
          this._peer = serverSocket;
          serverSocket._peer = this;
          const pendingWrites = this._pendingWrites.splice(0);
          httpServer.__quenchRawConnection?.(serverSocket);
          __quenchDeliverPendingWrites(serverSocket, pendingWrites);
          return;
        }
        if (!server) {
          const address =
            this._resolvedAddress ||
            _options.host ||
            (isIPv6(_options.family) ? "::1" : "127.0.0.1");
          const error = new Error(
            `connect ECONNREFUSED ${address}:${requestedPort}`
          );
          error.code = "ECONNREFUSED";
          error.address = address;
          error.port = requestedPort;
          this.emit("error", error);
          return;
        }
        if (
          Number.isFinite(server.maxConnections) &&
          server.maxConnections >= 0 &&
          server._connections.size >= server.maxConnections
        ) {
          const error = new Error("socket hang up");
          error.code = "ECONNRESET";
          queueMicrotask(() => {
            if (!this.destroyed && this.listenerCount("error") > 0) {
              this.emit("error", error);
            }
            this.destroy();
          });
          return;
        }
        const serverSocket = new __quenchNetModule.Socket();
        serverSocket._handle = { setKeepAlive: () => {} };
        serverSocket.allowHalfOpen = server._allowHalfOpen;
        if (server.keepAlive !== undefined) {
          serverSocket.setKeepAlive(
            server.keepAlive,
            server.keepAliveInitialDelay
          );
        }
        this._peer = serverSocket;
        serverSocket._peer = this;
        const pendingWrites = this._pendingWrites.splice(0);
        server._connections.add(serverSocket);
        serverSocket.once("close", () => {
          server._connections.delete(serverSocket);
          server._finishClose?.();
        });
        server.emit("connection", serverSocket);
        __quenchDeliverPendingWrites(serverSocket, pendingWrites);
      });
    }
    return this;
  }
  write(_data, encoding, callback) {
    if (typeof encoding === "function") {
      callback = encoding;
      encoding = undefined;
    }
    const length =
      typeof _data === "string"
        ? ["latin1", "binary", "ascii"].includes(encoding)
          ? _data.length
          : NodeBuffer.byteLength(_data, encoding)
        : _data?.byteLength || _data?.length || 0;
    if (!this.destroyed) {
      const bytes =
        typeof _data === "string"
          ? ["latin1", "binary", "ascii"].includes(encoding)
            ? Array.from(_data, (value) => value.charCodeAt(0) & 0xff)
            : Array.from(new TextEncoder().encode(_data))
          : Array.from(new Uint8Array(_data.buffer || _data));
      if (this._nativeId) {
        __quench_tcp_write(this._nativeId, bytes);
      } else if (!this._peer && bytes.length) {
        this._pendingWrites.push(bytes);
      } else if (this._peer && !this._peer.destroyed && bytes.length) {
        const peer = this._peer;
        const chunk = NodeBuffer.from(bytes);
        queueMicrotask(() => {
          if (peer.destroyed) return;
          peer.bytesRead += chunk.length;
          const delivered = peer.encoding
            ? chunk.toString(peer.encoding)
            : chunk;
          if (peer._paused || peer.listenerCount("data") === 0) {
            peer._pendingData.push(delivered);
          } else peer.emit("data", delivered);
        });
      }
      this._bufferSize += length;
    }
    if (length) this.bytesWritten += length;
    if (this.destroyed && typeof callback === "function") {
      const error = new Error("Cannot call write after a stream was destroyed");
      error.code = "ERR_STREAM_DESTROYED";
      queueMicrotask(() => callback(error));
    } else {
      if (typeof callback === "function") queueMicrotask(callback);
    }
    return true;
  }
  end(_data, callback) {
    if (typeof _data === "function") {
      callback = _data;
      _data = undefined;
    }
    if (this.connecting) {
      this._endPending = true;
      if (typeof callback === "function") queueMicrotask(callback);
      return this;
    }
    const peer = this._peer;
    if (_data !== undefined && _data !== null && _data !== "") {
      this.write(_data);
    }
    if (typeof callback === "function") queueMicrotask(callback);
    if (this._nativeId) {
      __quench_tcp_shutdown(this._nativeId);
      this.writable = false;
      this.readyState = "readOnly";
      queueMicrotask(() => this.emit("finish"));
      return this;
    }
    this.writable = false;
    this._localEnded = true;
    this.readyState = "readOnly";
    queueMicrotask(() => {
      this._bufferSize = 0;
      if (!this.__finishEmitted) {
        this.__finishEmitted = true;
        this.emit("finish");
      }
      if (peer && !peer.destroyed) {
        queueMicrotask(() => {
          peer._readableEnded = true;
          peer.emit("end");
          if (!peer.allowHalfOpen) {
            peer.writable = false;
            peer._localEnded = true;
            if (!peer.__finishEmitted) {
              peer.__finishEmitted = true;
              peer.emit("finish");
            }
            peer.destroy();
          }
          if (this._localEnded && peer._localEnded) this.destroy();
        });
      }
    });
    return this;
  }
};
"#);
