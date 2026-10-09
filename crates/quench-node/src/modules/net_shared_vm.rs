use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver, TryRecvError};

const FIRST_TRANSPORT_ID: u64 = 1;
const TCP_READ_CHUNK: usize = 16 * 1024;
const TCP_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const NET_MODULE_FACTORY: &str = r#"((Duplex, connectOperation, writeOperation, endOperation, destroyOperation, encodingOperation, listenOperation, closeOperation, addressOperation) => {
  const invalidArgValue = (value, message) => {
    const error = new TypeError(message || `The argument 'options' is invalid. Received ${String(value)}`);
    error.code = 'ERR_INVALID_ARG_VALUE'; return error;
  };
  const invalidArgType = (value) => {
    const error = new TypeError(`The "port" argument must be of type number. Received ${String(value)}`);
    error.code = 'ERR_INVALID_ARG_TYPE'; return error;
  };
  const validatePort = (value, allowZero, connecting = false) => {
    if (value === undefined) { if (allowZero && !connecting) return 0; throw invalidArgType(value); }
    if (allowZero && !connecting && value === null) return 0;
    if (typeof value !== 'number' && typeof value !== 'string') {
      throw connecting ? invalidArgType(value) : invalidArgValue(value);
    }
    const port = typeof value === 'string' ? (value.trim() === '' ? Number.NaN : Number(value)) : value;
    if (!Number.isFinite(port) || !Number.isInteger(port) || port < (allowZero ? 0 : 1) || port > 65535) {
      const error = new RangeError(`Port should be >= ${allowZero ? 0 : 1} and < 65536. Received ${String(value)}`);
      error.code = 'ERR_SOCKET_BAD_PORT'; throw error;
    }
    return port;
  };
  const listeners = (socket) => socket._listeners;
  class Socket extends Duplex {
    constructor(options = {}) {
      super({
        ...options, autoDestroy: true, allowHalfOpen: true,
        read() {},
        write(chunk, encoding, callback) { try { writeOperation(this, chunk); callback(); } catch (error) { callback(error); } },
        final(callback) { try { endOperation(this); callback(); } catch (error) { callback(error); } },
        destroy(error, callback) { try { destroyOperation(this, error); callback(error); } catch (failure) { callback(failure); } },
      });
      this.allowHalfOpen = false;
      this._encoding = null;
      this._handle = options.handle || null;
      this._noDelay = Boolean(options.noDelay);
      this.connecting = false;
      this.pending = true;
      this.bytesRead = 0;
      this.bytesWritten = 0;
      this._quenchPreconnectWrites = [];
      this._quenchPreconnectEnd = null;
      this._quenchConnectingEmit = false;
      this._quenchTimeout = 0;
      this._quenchTimeoutTimer = null;
      this.on('close', () => {
        this.pending = true;
        if (this._quenchTimeoutTimer !== null) clearTimeout(this._quenchTimeoutTimer);
        this._quenchTimeoutTimer = null;
      });
      this.on('data', (chunk) => {
        if (chunk) this.bytesRead += chunk.length || 0;
        this._quenchResetTimeout();
      });
    }
    emit(event, ...args) {
      if (event === 'connect') {
        this.connecting = false;
        this.pending = false;
        this._quenchResetTimeout();
        this._quenchConnectingEmit = true;
        const result = super.emit(event, ...args);
        this._quenchConnectingEmit = false;
        for (const queued of this._quenchPreconnectWrites.splice(0)) super.write(...queued);
        if (this._quenchPreconnectEnd) super.end(...this._quenchPreconnectEnd);
        this._quenchPreconnectEnd = null;
        return result;
      }
      if (event === 'close') { this.pending = true; if (this._quenchTimeoutTimer !== null) clearTimeout(this._quenchTimeoutTimer); this._quenchTimeoutTimer = null; }
      if (event === 'data' && args[0]) { this.bytesRead += args[0].length || 0; this._quenchResetTimeout(); }
      if (event === 'end' && !this.allowHalfOpen) {
        const result = super.emit(event, ...args);
        this.end();
        return result;
      }
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
    address() {
      if (!this.localAddress) return undefined;
      return { address: this.localAddress, family: this.localFamily || 'IPv4', port: this.localPort || 0 };
    }
    setNoDelay(noDelay = true) {
      const enable = Boolean(noDelay);
      if (!this._handle) {
        this._noDelay = enable;
        return this;
      }
      if (typeof this._handle.setNoDelay === 'function' && enable !== this._noDelay) {
        this._noDelay = enable;
        this._handle.setNoDelay(enable);
      }
      return this;
    }
    setKeepAlive(enable = false, initialDelay = 0) {
      if (this._handle && typeof this._handle.setKeepAlive === 'function') this._handle.setKeepAlive(!!enable, initialDelay);
      return this;
    }
    setTimeout(timeout, callback) {
      if (typeof timeout !== 'number' || !Number.isFinite(timeout) || timeout < 0) throw new TypeError('The "msecs" argument must be a non-negative number');
      if (typeof callback === 'function') this.once('timeout', callback);
      this._quenchTimeout = timeout;
      this._quenchResetTimeout();
      return this;
    }
    _quenchResetTimeout() {
      if (this._quenchTimeoutTimer !== null) clearTimeout(this._quenchTimeoutTimer);
      this._quenchTimeoutTimer = null;
      if (this._quenchTimeout > 0 && !this.destroyed && !this.connecting) {
        this._quenchTimeoutTimer = setTimeout(() => this.emit('timeout'), this._quenchTimeout);
      }
    }
    write(chunk, encoding, callback) {
      if (typeof encoding === 'function') { callback = encoding; encoding = undefined; }
      this._quenchResetTimeout();
      this.bytesWritten += typeof chunk === 'string' ? Buffer.byteLength(chunk, encoding || 'utf8') : (chunk && chunk.length || 0);
      if ((!this.__quenchNetSocketId && !this.connecting) || this.connecting || this._quenchConnectingEmit) {
        this._quenchPreconnectWrites.push([chunk, encoding, callback]);
        return this.writableHighWaterMark !== 0;
      }
      return super.write(chunk, encoding, callback);
    }
    end(chunk, encoding, callback) {
      if (typeof chunk === 'function') { callback = chunk; chunk = undefined; encoding = undefined; }
      else if (typeof encoding === 'function') { callback = encoding; encoding = undefined; }
      if (chunk !== undefined && chunk !== null) this.bytesWritten += typeof chunk === 'string' ? Buffer.byteLength(chunk, encoding || 'utf8') : (chunk.length || 0);
      if ((!this.__quenchNetSocketId && !this.connecting) || this.connecting || this._quenchConnectingEmit) {
        this._quenchPreconnectEnd = [chunk, encoding, callback];
        return this;
      }
      return super.end(chunk, encoding, callback);
    }
    connect(port, host, callback) {
      let blockList;
      if (port && typeof port === 'object') {
        const options = port;
        callback = typeof host === 'function' ? host : callback;
        host = options.host ?? options.hostname;
        blockList = options.blockList;
        if (options.hints !== undefined && (!Number.isInteger(options.hints) || (options.hints & ~49) !== 0)) {
          const error = new TypeError(`The argument 'hints' is invalid. Received ${String(options.hints)}`);
          error.code = 'ERR_INVALID_ARG_VALUE'; throw error;
        }
        port = options.port;
      }
      if (typeof host === 'function') { callback = host; host = undefined; }
      port = validatePort(port, true, true);
      if (typeof callback === 'function') this.once('connect', callback);
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
      connectOperation(this, port, host);
      return this;
    }
    ref() { return this; }
    unref() { return this; }
  }
  const connect = function connect(port, host, callback) {
    const socket = new Socket();
    return socket.connect(port, host, callback);
  };
  function Server(options, listener) {
    if (!(this instanceof Server)) return new Server(options, listener);
    this._listeners = new Map();
    this.listening = false;
    if (typeof options === 'function') listener = options;
    if (typeof listener === 'function') this.on('connection', listener);
  }
  Server.prototype.on = function(event, callback) { const list = this._listeners.get(event) || []; list.push(callback); this._listeners.set(event, list); return this; };
  Server.prototype.addListener = function(event, callback) { return this.on(event, callback); };
  Server.prototype.once = function(event, callback) { const wrapped = (...args) => { this.removeListener(event, wrapped); callback.apply(this, args); }; wrapped.listener = callback; return this.on(event, wrapped); };
  Server.prototype.removeListener = function(event, callback) { const list = this._listeners.get(event) || []; this._listeners.set(event, list.filter((item) => item !== callback && item.listener !== callback)); return this; };
  Server.prototype.off = Server.prototype.removeListener;
  Server.prototype.listeners = function(event) { return (this._listeners.get(event) || []).map((item) => item.listener || item); };
  Server.prototype.listenerCount = function(event) { return this.listeners(event).length; };
  Server.prototype.removeAllListeners = function(event) { if (event === undefined) this._listeners.clear(); else this._listeners.delete(event); return this; };
  Server.prototype.emit = function(event, ...args) { for (const callback of [...(this._listeners.get(event) || [])]) callback.apply(this, args); return true; };
  Server.prototype.listen = function(port, host, callback) {
      if (typeof port === 'function') { callback = port; port = 0; }
      if (port && typeof port === 'object') {
        const options = port;
        callback = typeof host === 'function' ? host : callback;
        if (!('port' in options) && !('path' in options)) {
          throw invalidArgValue(options, `The argument 'options' must have the property "port" or "path". Received ${String(options)}`);
        }
        if ('path' in options && !('port' in options)) throw invalidArgValue(options);
        host = options.host;
        port = options.port;
      }
      if (typeof host === 'function') { callback = host; host = undefined; }
      port = validatePort(port, true);
      if (typeof callback === 'function') this.once('listening', callback);
      listenOperation(this, port, host);
      this.listening = true;
      return this;
    };
  Server.prototype.address = function() { return addressOperation(this); };
  Server.prototype.close = function(callback) { if (typeof callback === 'function') this.once('close', callback); closeOperation(this); this.listening = false; return this; };
  Server.prototype.ref = function() { return this; };
  Server.prototype.unref = function() { return this; };
  const createServer = function createServer(listener) { return new Server(listener); };
  function Stream(options) { return new Socket(options); }
  Stream.prototype = Socket.prototype;
  Object.setPrototypeOf(Stream, Socket);
  const blockError = (code, message) => Object.assign(
    code === 'ERR_OUT_OF_RANGE' ? new RangeError(`${code}: ${message}`) : new TypeError(`${code}: ${message}`),
    { code },
  );
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
      this[blockListHandleKey] = { check: (address) => this.check(address?.address, address?.family) };
    }
    _insert(kind, first, last = first, prefix) {
      if (this._rules.some((rule) => rule.kind === kind && rule.family === first.family && rule.start === first.value && rule.end === last.value)) return;
      this._rules.push({ kind, family: first.family, start: first.value, end: last.value, address: first.text, endAddress: last.text, bits: first.bits, prefix });
    }
    addAddress(address, type) { const first = addressValue(address, type); this._insert('Address', first); }
    addAddresses(addresses, type) {
      if (!Array.isArray(addresses)) throw blockError('ERR_INVALID_ARG_TYPE', 'The "addresses" argument must be an array');
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
      if (!Array.isArray(cidrs)) throw blockError('ERR_INVALID_ARG_TYPE', 'The "cidrs" argument must be an array');
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
  return { Socket, Stream, Server, BlockList, SocketAddress, connect, createConnection: connect, createServer };
})"#;

/// TCP resources for the shared-VM projection. Protocol modules keep only
/// endpoint IDs and parsed state; this is the one owner of socket bytes and
/// OS handles for that execution path.
pub(crate) struct Transport {
    next_id: u64,
    listeners: HashMap<u64, TcpListener>,
    sockets: HashMap<u64, SharedSocket>,
    connecting: HashMap<u64, Receiver<Result<TcpStream, String>>>,
}

struct SharedSocket {
    stream: TcpStream,
    pending_write: Vec<u8>,
    write_offset: usize,
    end_after_write: bool,
    read_ended: bool,
    write_ended: bool,
}

pub(crate) enum TransportEvent {
    Accepted {
        listener: u64,
        socket: u64,
        remote: SocketAddr,
    },
    Connected { socket: u64 },
    ConnectError { socket: u64, message: String },
    Data { socket: u64, bytes: Vec<u8> },
    End { socket: u64 },
    Closed { socket: u64 },
    Error { socket: u64, message: String },
}

impl Transport {
    pub(crate) fn new() -> Self {
        Self {
            next_id: FIRST_TRANSPORT_ID,
            listeners: HashMap::new(),
            sockets: HashMap::new(),
            connecting: HashMap::new(),
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

pub(crate) fn listen(transport: &mut Transport, address: SocketAddr) -> Result<u64, String> {
    let listener = TcpListener::bind(address).map_err(|error| error.to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let id = transport.id()?;
    transport.listeners.insert(id, listener);
    Ok(id)
}

pub(crate) fn connect(transport: &mut Transport, host: String, port: u16) -> Result<u64, String> {
    let id = transport.id()?;
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name(format!("quench-node-tcp-{id}"))
        .spawn(move || {
            let result = connect_with_timeout(&host, port).and_then(|stream| {
                stream
                    .set_nonblocking(true)
                    .map(|()| stream)
                    .map_err(|error| error.to_string())
            });
            let _ = sender.send(result);
        })
        .map_err(|error| format!("cannot start TCP connect: {error}"))?;
    transport.connecting.insert(id, receiver);
    Ok(id)
}

pub(crate) fn address(transport: &Transport, listener: u64) -> Option<SocketAddr> {
    transport
        .listeners
        .get(&listener)
        .and_then(|listener| listener.local_addr().ok())
}

pub(crate) fn write(transport: &mut Transport, socket: u64, bytes: &[u8]) -> Result<(), String> {
    let stream = transport
        .sockets
        .get_mut(&socket)
        .ok_or_else(|| "shared TCP socket is not connected".to_owned())?;
    stream.pending_write.extend_from_slice(bytes);
    Ok(())
}

pub(crate) fn is_connected(transport: &Transport, socket: u64) -> bool {
    transport.sockets.contains_key(&socket)
}

pub(crate) fn end(transport: &mut Transport, socket: u64) -> Result<(), String> {
    if let Some(stream) = transport.sockets.get_mut(&socket) {
        stream.end_after_write = true;
    }
    Ok(())
}

pub(crate) fn close_listener(transport: &mut Transport, listener: u64) {
    transport.listeners.remove(&listener);
}

pub(crate) fn close_socket(transport: &mut Transport, socket: u64) {
    transport.connecting.remove(&socket);
    transport.sockets.remove(&socket);
}

pub(crate) fn poll(transport: &mut Transport) -> Vec<TransportEvent> {
    let mut events = poll_connects(transport);
    events.extend(poll_accepts(transport));
    events.extend(poll_sockets(transport));
    events
}

pub(crate) fn has_work(transport: &Transport) -> bool {
    !transport.listeners.is_empty()
        || !transport.sockets.is_empty()
        || !transport.connecting.is_empty()
}

pub(crate) fn cleanup(transport: &mut Transport) {
    transport.listeners.clear();
    transport.sockets.clear();
    transport.connecting.clear();
}

pub(crate) const fn poll_interval() -> std::time::Duration {
    std::time::Duration::from_millis(1)
}

fn poll_connects(transport: &mut Transport) -> Vec<TransportEvent> {
    let mut events = Vec::new();
    let ids = transport.connecting.keys().copied().collect::<Vec<_>>();
    for id in ids {
        let Some(receiver) = transport.connecting.get(&id) else {
            continue;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => {
                continue;
            }
            Err(TryRecvError::Disconnected) => Err("TCP connect worker stopped".to_owned()),
        };
        transport.connecting.remove(&id);
        match result {
            Ok(stream) => {
                transport.sockets.insert(
                    id,
                    SharedSocket {
                        stream,
                        pending_write: Vec::new(),
                        write_offset: 0,
                        end_after_write: false,
                        read_ended: false,
                        write_ended: false,
                    },
                );
                events.push(TransportEvent::Connected { socket: id });
            }
            Err(message) => events.push(TransportEvent::ConnectError {
                socket: id,
                message,
            }),
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
            loop {
                match listener.accept() {
                    Ok((stream, remote)) => accepted.push((*listener_id, stream, remote)),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => break,
                }
            }
            accepted
        })
        .collect::<Vec<_>>();
    let mut events = Vec::with_capacity(listeners.len());
    for (listener, stream, remote) in listeners {
        if stream.set_nonblocking(true).is_err() {
            continue;
        }
        let Ok(socket) = transport.id() else {
            continue;
        };
        transport.sockets.insert(
            socket,
            SharedSocket {
                stream,
                pending_write: Vec::new(),
                write_offset: 0,
                end_after_write: false,
                read_ended: false,
                write_ended: false,
            },
        );
        events.push(TransportEvent::Accepted {
            listener,
            socket,
            remote,
        });
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
        if socket.write_offset < socket.pending_write.len() {
            match socket
                .stream
                .write(&socket.pending_write[socket.write_offset..])
            {
                Ok(0) => {
                    events.push(TransportEvent::Error {
                        socket: id,
                        message: std::io::Error::from(std::io::ErrorKind::WriteZero).to_string(),
                    });
                    terminal.push(id);
                    continue;
                }
                Ok(written) => socket.write_offset += written,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => {
                    events.push(TransportEvent::Error {
                        socket: id,
                        message: error.to_string(),
                    });
                    terminal.push(id);
                    continue;
                }
            }
            if socket.write_offset == socket.pending_write.len() {
                socket.pending_write.clear();
                socket.write_offset = 0;
            }
        }
        if socket.end_after_write && socket.pending_write.is_empty() {
            let _ = socket.stream.shutdown(std::net::Shutdown::Write);
            socket.end_after_write = false;
            socket.write_ended = true;
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
                Ok(count) => received.extend_from_slice(&buffer[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => {
                    failure = Some(error.to_string());
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
        if let Some(message) = failure {
            events.push(TransportEvent::Error {
                socket: id,
                message,
            });
            terminal.push(id);
        } else if ended {
            events.push(TransportEvent::End { socket: id });
        }
        if socket.read_ended && socket.write_ended {
            events.push(TransportEvent::Closed { socket: id });
            terminal.push(id);
        }
    }
    for id in terminal {
        transport.sockets.remove(&id);
    }
    events
}

fn connect_with_timeout(host: &str, port: u16) -> Result<TcpStream, String> {
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|_| format!("getaddrinfo ENOTFOUND {host}"))?
        .collect::<Vec<_>>();
    let mut last_error = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, TCP_CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(error) => {
                let code = match error.kind() {
                    std::io::ErrorKind::ConnectionRefused => "ECONNREFUSED",
                    std::io::ErrorKind::TimedOut => "ETIMEDOUT",
                    std::io::ErrorKind::AddrNotAvailable => "EADDRNOTAVAIL",
                    std::io::ErrorKind::PermissionDenied => "EACCES",
                    _ => "ENETUNREACH",
                };
                last_error = Some(format!("connect {code} {address} ({error})"));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| format!("getaddrinfo ENOTFOUND {host}")))
}

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
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
    let connect = context.host_function(crate::host::shared_vm::operation("netConnect"))?;
    let write = context.host_function(crate::host::shared_vm::operation("netSocketWrite"))?;
    let end = context.host_function(crate::host::shared_vm::operation("netSocketEnd"))?;
    let destroy = context.host_function(crate::host::shared_vm::operation("netSocketDestroy"))?;
    let encoding = context.host_function(crate::host::shared_vm::operation("netSocketSetEncoding"))?;
    let listen = context.host_function(crate::host::shared_vm::operation("netServerListen"))?;
    let close = context.host_function(crate::host::shared_vm::operation("netServerClose"))?;
    let address = context.host_function(crate::host::shared_vm::operation("netServerAddress"))?;
    let stream = crate::host::shared_vm::commonjs::stream_module(context)?;
    let duplex_key = context.string_rooted("Duplex");
    let duplex = context.get_property_rooted(stream, duplex_key)?;
    let factory = context.evaluate_script_rooted(NET_MODULE_FACTORY, "node:net/module.js")?;
    let undefined = context.undefined();
    let surface = context.call_rooted(
        factory,
        undefined,
        &[duplex, connect, write, end, destroy, encoding, listen, close, address],
    )?;
    for name in ["Socket", "Stream", "Server", "BlockList", "SocketAddress", "connect", "createConnection", "createServer"] {
        let key = context.string_rooted(name);
        let value = context.get_property_rooted(surface, key)?;
        if !context.set_property_rooted(module, key, value, module)? {
            return Err(RootedError::host(format!("cannot install net.{name}")));
        }
    }
    let socket_key = context.string_rooted("Socket");
    let socket_constructor = context.get_property_rooted(surface, socket_key)?;
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
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = args.first().copied().and_then(|root| context.to_string(root).ok());
    let family = input.as_deref().and_then(parse_ip);
    Ok(context.number(match family {
        Some(IpAddr::V4(_)) => 4.0,
        Some(IpAddr::V6(_)) => 6.0,
        None => 0.0,
    }))
}

pub(crate) fn is_ipv4(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = args.first().copied().and_then(|root| context.to_string(root).ok());
    let is_ipv4 = input.is_some_and(|input| input.parse::<Ipv4Addr>().is_ok());
    Ok(context.boolean(is_ipv4))
}

pub(crate) fn is_ipv6(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = args.first().copied().and_then(|root| context.to_string(root).ok());
    let is_ipv6 = input.as_deref().and_then(parse_ip).is_some_and(|ip| ip.is_ipv6());
    Ok(context.boolean(is_ipv6))
}

fn parse_ip(input: &str) -> Option<IpAddr> {
    if let Ok(address) = input.parse::<IpAddr>() {
        return Some(address);
    }
    let (address, zone) = input.rsplit_once('%')?;
    if zone.is_empty()
        || !zone
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
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
    let port = args
        .get(1)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_number)
        .filter(|port| port.fract() == 0.0 && (0.0..=65535.0).contains(port))
        .ok_or_else(|| RootedError::host("invalid TCP port"))? as u16;
    let host = match args.get(2).copied() {
        Some(host) => context.string_text(host)?.unwrap_or_else(|| "localhost".to_owned()),
        None => "localhost".to_owned(),
    };
    let shared = context.host_mut().shared_state();
    let id = crate::modules::net_shared_vm::connect(&mut shared.borrow_mut().tcp, host, port)
        .map_err(RootedError::host)?;
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
        },
    );
    Ok(socket)
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
    let bytes = args.get(1).copied().map(|value| {
        context
            .view_bytes_rooted(value)
            .unwrap_or_else(|| context.to_string(value).unwrap_or_default().into_bytes())
    }).unwrap_or_default();
    context
        .host_mut()
        .net_pending_writes
        .push((id, bytes));
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
    let id = socket_id(context, socket)?;
    context.host_mut().net_pending_ends.push(id);
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
    let id = match socket_id(context, socket) {
        Ok(id) => id,
        Err(_) => return Ok(socket),
    };
    context.host_mut().net_pending_writes.retain(|(socket, _)| *socket != id);
    context.host_mut().net_pending_ends.retain(|socket| *socket != id);
    let state = context.host_mut().shared_state();
    close_socket(&mut state.borrow_mut().tcp, id);
    context.host_mut().net_pending_destroys.push(id);
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
    let id = socket_id(context, socket)?;
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

pub(crate) fn server_listen_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let server = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("net.Server.listen requires a server"))?;
    let port = args
        .get(1)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_number)
        .filter(|port| port.fract() == 0.0 && (0.0..=65535.0).contains(port))
        .ok_or_else(|| RootedError::host("invalid TCP port"))? as u16;
    let host = match args.get(2).copied() {
        Some(host) => context.string_text(host)?.unwrap_or_else(|| "127.0.0.1".to_owned()),
        None => "127.0.0.1".to_owned(),
    };
    let address = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|error| RootedError::host(error.to_string()))?
        .next()
        .ok_or_else(|| RootedError::host("net.Server.listen host did not resolve"))?;
    let shared = context.host_mut().shared_state();
    let listener = listen(&mut shared.borrow_mut().tcp, address).map_err(RootedError::host)?;
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
    Ok(server)
}

pub(crate) fn server_close_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let server = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("net.Server.close requires a server"))?;
    let id = server_id(context, server)?;
    let shared = context.host_mut().shared_state();
    {
        let mut state = shared.borrow_mut();
        if let Some(listener) = state.net_servers.get_mut(&id).map(|server| {
            server.closing = true;
            server.listener
        }) {
            close_listener(&mut state.tcp, listener);
        }
    }
    Ok(server)
}

pub(crate) fn server_address_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let server = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("net.Server.address requires a server"))?;
    let id = server_id(context, server)?;
    let shared = context.host_mut().shared_state();
    let address = address(&shared.borrow().tcp, id);
    let Some(address) = address else {
        return Ok(context.null());
    };
    let object = context.object_rooted()?;
    let address_value = context.string_rooted(&address.ip().to_string());
    let key = context.string_rooted("address");
    context.set_property_rooted(object, key, address_value, object)?;
    let family = context.string_rooted(if address.ip().is_ipv4() { "IPv4" } else { "IPv6" });
    let key = context.string_rooted("family");
    context.set_property_rooted(object, key, family, object)?;
    let port = context.number(address.port() as f64);
    let key = context.string_rooted("port");
    context.set_property_rooted(object, key, port, object)?;
    Ok(object)
}

fn server_id(context: &mut NativeContext<'_, NodeHost>, server: RootId) -> Result<u64, RootedError> {
    let key = context.string_rooted("__quenchNetServerId");
    let value = context.get_property_rooted(server, key)?;
    context
        .rooted_value(value)
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && id.fract() == 0.0 && *id >= 0.0)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("net server identifier is invalid"))
}

fn socket_id(context: &mut NativeContext<'_, NodeHost>, socket: RootId) -> Result<u64, RootedError> {
    let key = context.string_rooted("__quenchNetSocketId");
    let value = context.get_property_rooted(socket, key)?;
    context
        .rooted_value(value)
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && id.fract() == 0.0 && *id >= 0.0)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("net socket identifier is invalid"))
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
    let value = args.first().copied().and_then(|arg| {
        context
            .rooted_value(arg)
            .and_then(|value| value.as_number())
    });
    let Some(value) =
        value.filter(|value| value.is_finite() && value.fract() == 0.0 && *value > 0.0)
    else {
        let error = context.error_rooted("The \"ms\" argument must be a positive integer")?;
        let name = context.string_rooted("name");
        let range_error = context.string_rooted("RangeError");
        if !context.set_property_rooted(error, name, range_error, error)? {
            return Err(RootedError::host("cannot set net timeout error name"));
        }
        let code = context.string_rooted("code");
        let code_value = context.string_rooted("ERR_OUT_OF_RANGE");
        if !context.set_property_rooted(error, code, code_value, error)? {
            return Err(RootedError::host("cannot set net timeout error code"));
        }
        return Err(context.throw(error));
    };
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .net_auto_select_family_attempt_timeout =
        crate::modules::net_config::normalize_auto_select_family_attempt_timeout(value as u64);
    Ok(context.undefined())
}
