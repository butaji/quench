const isIPv4Part = (part) => {
  if (!/^\d+$/.test(part)) return false;
  const n = Number(part);
  if (n < 0 || n > 255) return false;
  if (part.length > 1 && part.startsWith("0")) return false;
  return part.length <= 3;
};
const __quenchNetServers = new Set();
const isIPv4 = (input) => {
  if (input == null) return false;
  if (typeof input !== "string") {
    try {
      return isIPv4(String(input));
    } catch {
      return false;
    }
  }
  const parts = input.split(".");
  if (parts.length !== 4) return false;
  return parts.every(isIPv4Part);
};
const validateIPv6Group = (group) => {
  if (group.includes(".")) {
    const parts = group.split(".");
    return parts.length === 4 && parts.every(isIPv4Part) ? 2 : 0;
  }
  if (group.length === 0 || group.length > 4 || !/^[0-9a-fA-F]+$/.test(group)) {
    return 0;
  }
  return 1;
};
const normalizeIPv6Zone = (input) => {
  if (!input.includes("%")) return input;
  const percentIndex = input.indexOf("%");
  const zone = input.slice(percentIndex + 1);
  if (!isValidIPv6Zone(input, percentIndex, zone)) return null;
  const address = input.slice(0, percentIndex);
  return address.length ? address : null;
};
const isValidIPv6Zone = (input, percentIndex, zone) =>
  percentIndex !== input.length - 1 &&
  input.indexOf("%", percentIndex + 1) === -1 &&
  !zone.includes(":") &&
  !zone.includes("%") &&
  !zone.includes("@") &&
  /^[0-9A-Za-z._-]+$/.test(zone);
const isValidIPv6GroupPosition = (
  group,
  index,
  groups,
  hasDoubleColon,
  isHead,
) => {
  if (isHead && hasDoubleColon && group.includes(".")) return false;
  if (
    !hasDoubleColon &&
    isHead &&
    group.includes(".") &&
    index < groups.length - 1
  ) {
    return false;
  }
  return !(!isHead && index < groups.length - 1 && group.includes("."));
};
const countIPv6GroupList = (groups, hasDoubleColon, isHead) => {
  let expanded = 0;
  for (let index = 0; index < groups.length; index++) {
    const group = groups[index];
    if (
      !isValidIPv6GroupPosition(group, index, groups, hasDoubleColon, isHead)
    ) {
      return 0;
    }
    const width = validateIPv6Group(group);
    if (!width) return 0;
    expanded += width;
  }
  return expanded;
};
const countIPv6Groups = (headGroups, tailGroups, hasDoubleColon) => {
  return (
    countIPv6GroupList(headGroups, hasDoubleColon, true) +
    countIPv6GroupList(tailGroups, hasDoubleColon, false)
  );
};
const parseIPv6Groups = (address) => {
  if (address === "::") return { expanded: 0, special: true };
  const doubleColonIndex = address.indexOf("::");
  if (
    doubleColonIndex !== -1 &&
    address.indexOf("::", doubleColonIndex + 1) !== -1
  ) {
    return null;
  }
  const hasDoubleColon = doubleColonIndex !== -1;
  const head = hasDoubleColon ? address.slice(0, doubleColonIndex) : address;
  const tail = hasDoubleColon ? address.slice(doubleColonIndex + 2) : "";
  const headGroups = head === "" ? [] : head.split(":");
  const tailGroups = tail === "" ? [] : tail.split(":");
  const expanded = countIPv6Groups(headGroups, tailGroups, hasDoubleColon);
  return { expanded, hasDoubleColon, special: false };
};
const isIPv6 = (input) => {
  if (input == null) return false;
  if (typeof input !== "string") {
    try {
      return isIPv6(String(input));
    } catch {
      return false;
    }
  }
  return isIPv6String(input);
};
const isIPv6String = (input) => {
  if (input.length === 0) return false;
  const address = normalizeIPv6Zone(input);
  if (!address) return false;
  const parsed = parseIPv6Groups(address);
  if (!parsed || parsed.special) return Boolean(parsed);
  if (!parsed.expanded) return false;
  return parsed.hasDoubleColon ? parsed.expanded <= 7 : parsed.expanded === 8;
};
const isIP = (input) => {
  if (input == null) return 0;
  if (typeof input !== "string") {
    try {
      return isIP(String(input));
    } catch {
      return 0;
    }
  }
  if (isIPv4(input)) return 4;
  if (isIPv6(input)) return 6;
  return 0;
};
const compareV4 = (a, b) => {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < 4; i++) {
    if (pa[i] !== pb[i]) return pa[i] - pb[i];
  }
  return 0;
};
const compareV6 = (a, b) => {
  const expand = (s) => {
    const dc = s.indexOf("::");
    if (dc === -1) return s.split(":");
    const head = s.slice(0, dc) || "";
    const tail = s.slice(dc + 2) || "";
    const h = head === "" ? [] : head.split(":");
    const t = tail === "" ? [] : tail.split(":");
    const fill = 8 - h.length - t.length;
    return [...h, ...Array(fill).fill("0"), ...t];
  };
  const ea = expand(a).map((x) => parseInt(x, 16));
  const eb = expand(b).map((x) => parseInt(x, 16));
  for (let i = 0; i < 8; i++) {
    if (ea[i] !== eb[i]) return ea[i] - eb[i];
  }
  return 0;
};
const matchSubnetV4 = (addr, net, prefix) => {
  const a = addr.split(".").map(Number);
  const n = net.split(".").map(Number);
  if (a.length !== 4 || n.length !== 4) return false;
  const mask = prefix === 0 ? 0 : (0xffffffff << (32 - prefix)) >>> 0;
  const ai = ((a[0] << 24) | (a[1] << 16) | (a[2] << 8) | a[3]) >>> 0;
  const ni = ((n[0] << 24) | (n[1] << 16) | (n[2] << 8) | n[3]) >>> 0;
  return (ai & mask) === (ni & mask);
};
const expandV6 = (s) => {
  const dc = s.indexOf("::");
  if (dc === -1) return s.split(":");
  const head = s.slice(0, dc) || "";
  const tail = s.slice(dc + 2) || "";
  const h = head === "" ? [] : head.split(":");
  const t = tail === "" ? [] : tail.split(":");
  const fill = 8 - h.length - t.length;
  return [...h, ...Array(fill).fill("0"), ...t];
};
const matchSubnetV6 = (addr, net, prefix) => {
  const ea = expandV6(addr).map((x) => parseInt(x, 16));
  const en = expandV6(net).map((x) => parseInt(x, 16));
  if (ea.length !== 8 || en.length !== 8) return false;
  if (prefix === 0) return true;
  for (let i = 0; i < 8; i++) {
    if (prefix <= i * 16) break;
    const bits = Math.min(16, prefix - i * 16);
    const mask = bits === 16 ? 0xffff : (0xffff << (16 - bits)) & 0xffff;
    if ((ea[i] & mask) !== (en[i] & mask)) return false;
  }
  return true;
};
const normalizeAddress = (address, label = "address") => {
  if (typeof address === "string") return { value: address, explicit: false };
  if (address && typeof address.address === "string") {
    return {
      value: address.address,
      explicit: address.family !== undefined,
    };
  }
  const error = new TypeError(`Invalid ${label}`);
  error.code = "ERR_INVALID_ARG_TYPE";
  throw error;
};
const normalizeRangeEndpoint = (value, label) => {
  if (typeof value === "string") return value;
  if (value && typeof value.address === "string") return value.address;
  const error = new TypeError(`Invalid ${label}`);
  error.code = "ERR_INVALID_ARG_TYPE";
  throw error;
};
const resolveAddressType = (value, type, checkType) => {
  if (type !== undefined) return checkType(type);
  const resolved = isIPv4(value) ? "ipv4" : isIPv6(value) ? "ipv6" : null;
  if (resolved) return resolved;
  const error = new TypeError("Invalid address");
  error.code = "ERR_INVALID_ARG_VALUE";
  throw error;
};
const checkBlockListEntry = (entries, str, explicitType, inputKind) => {
  if (!entries.has(str)) return false;
  const entry = entries.get(str);
  return (
    explicitType ||
    inputKind === "socket" ||
    (inputKind === "string" && !entry.explicit)
  );
};
const checkBlockListV4 = (blockList, str, explicitType, inputKind) => {
  if (checkBlockListEntry(blockList._v4, str, explicitType, inputKind)) {
    return true;
  }
  for (const [start, end] of blockList._v4Ranges) {
    if (compareV4(str, start) >= 0 && compareV4(str, end) <= 0) return true;
  }
  for (const [net, prefix] of blockList._v4Subnets) {
    if (matchSubnetV4(str, net, prefix)) return true;
  }
  return blockList._v6.has("::ffff:" + str);
};
const checkMappedV4 = (blockList, value) => {
  const marker = value.indexOf("::ffff:");
  if (marker === -1) return false;
  const tail = value.slice(marker + 7);
  let v4 = tail;
  if (tail.indexOf(":") !== -1) {
    const parts = tail.split(":");
    if (
      parts.length !== 2 ||
      !/^[0-9a-fA-F]+$/.test(parts[0]) ||
      !/^[0-9a-fA-F]+$/.test(parts[1])
    ) {
      return false;
    }
    const first = parseInt(parts[0], 16);
    const second = parseInt(parts[1], 16);
    v4 = `${(first >> 8) & 0xff}.${first & 0xff}.${(second >> 8) & 0xff}.${
      second & 0xff
    }`;
  }
  return checkBlockListV4(blockList, v4, true, "socket");
};
const checkBlockListV6 = (blockList, str, explicitType, inputKind) => {
  if (checkBlockListEntry(blockList._v6, str, explicitType, inputKind)) {
    return true;
  }
  for (const [start, end] of blockList._v6Ranges) {
    if (compareV6(str, start) >= 0 && compareV6(str, end) <= 0) return true;
  }
  for (const [net, prefix] of blockList._v6Subnets) {
    if (matchSubnetV6(str, net, prefix)) return true;
  }
  return checkMappedV4(blockList, str);
};
const resolveBlockListCheck = (address, type, checkType) => {
  const str = normalizeRangeEndpoint(address, "address");
  const explicitType = type !== undefined;
  let resolvedType;
  if (type === undefined) {
    resolvedType = isIPv4(str) ? "ipv4" : isIPv6(str) ? "ipv6" : null;
  } else resolvedType = resolveAddressType(str, type, checkType);
  const inputKind = address &&
      typeof address !== "string" &&
      typeof address.address === "string"
    ? "socket"
    : "string";
  return { str, resolvedType, explicitType, inputKind };
};
const __quenchNetModule = {
  isIP,
  isIPv4,
  isIPv6,
  getDefaultAutoSelectFamily: () => false,
  setDefaultAutoSelectFamily: () => undefined,
  getDefaultAutoSelectFamilyAttemptTimeout: () => 250,
  Socket: class Socket extends globalThis.__nodeEventEmitter {
    constructor() {
      super();
      this.readable = true;
      this.writable = true;
      this.allowHalfOpen = false;
      this.destroyed = false;
      this._bufferSize = 0;
      this._handle = {};
    }
    get bufferSize() {
      return this._bufferSize;
    }
    setEncoding(encoding) {
      this.encoding = String(encoding);
      return this;
    }
    resume() {
      return this;
    }
    pause() {
      return this;
    }
    setNoDelay() {
      return this;
    }
    setKeepAlive() {
      return this;
    }
    address() {
      return this.destroyed ? null : undefined;
    }
    destroy() {
      this.destroyed = true;
      queueMicrotask(() => this.emit("close"));
      return this;
    }
    resetAndDestroy() {
      return this.destroy();
    }
    connect(_options, callback) {
      globalThis.__quenchValidateConnectionOptions(_options);
      if (typeof callback === "function") this.once("connect", callback);
      queueMicrotask(() => this.emit("connect"));
      return this;
    }
    write(_data, callback) {
      if (!this.destroyed) {
        this._bufferSize += typeof _data === "string"
          ? NodeBuffer.byteLength(_data)
          : _data?.byteLength || _data?.length || 0;
      }
      if (this.destroyed && typeof callback === "function") {
        const error = new Error(
          "Cannot call write after a stream was destroyed",
        );
        error.code = "ERR_STREAM_DESTROYED";
        queueMicrotask(() => callback(error));
      } else {
        if (typeof callback === "function") queueMicrotask(callback);
      }
      return true;
    }
    end(_data, callback) {
      if (typeof callback === "function") queueMicrotask(callback);
      queueMicrotask(() => {
        this._bufferSize = 0;
        this._handle = null;
        this.destroyed = true;
        this.emit("finish");
        this.emit("end");
        this.emit("close");
      });
      return this;
    }
  },
  createConnection: (options, callback) => {
    globalThis.__quenchValidateConnectionOptions(options);
    const socket = new __quenchNetModule.Socket();
    socket.connect(options, callback);
    queueMicrotask(() => {
      const server = [...__quenchNetServers].find((candidate) =>
        candidate.listening
      );
      if (server?._handler) server._handler(socket);
    });
    return socket;
  },
  setDefaultAutoSelectFamilyAttemptTimeout: () => undefined,
  BlockList: class BlockList {
    [Symbol.toStringTag] = "BlockList";
    constructor() {
      this._v4 = new Map();
      this._v6 = new Map();
      this._v4Ranges = [];
      this._v6Ranges = [];
      this._v4Subnets = [];
      this._v6Subnets = [];
      this._rules = [];
    }
    get rules() {
      return this._rules.slice().reverse();
    }
    [Symbol.for("nodejs.util.inspect.custom")](options) {
      return this;
    }
    _checkType(type) {
      if (typeof type !== "string") {
        const e = new TypeError("Invalid type [ERR_INVALID_ARG_TYPE]");
        e.code = "ERR_INVALID_ARG_TYPE";
        throw e;
      }
      const lower = type.toLowerCase();
      if (lower !== "ipv4" && lower !== "ipv6") {
        const e = new TypeError(
          `Invalid type '${type}' [ERR_INVALID_ARG_VALUE]`,
        );
        e.code = "ERR_INVALID_ARG_VALUE";
        throw e;
      }
      return lower;
    }
    addAddress(address, type) {
      const normalized = normalizeAddress(address);
      const str = normalized.value;
      const explicit = type !== undefined || normalized.explicit;
      const resolvedType = resolveAddressType(
        str,
        type,
        (value) => this._checkType(value),
      );
      if (resolvedType === "ipv4") {
        const existing = this._v4.get(str);
        this._v4.set(str, {
          explicit: (existing && existing.explicit) || explicit,
        });
        this._rules.push(`Address: IPv4 ${str}`);
      } else {
        const existing = this._v6.get(str);
        this._v6.set(str, {
          explicit: (existing && existing.explicit) || explicit,
        });
        this._rules.push(`Address: IPv6 ${str}`);
      }
    }
    addRange(start, end, type) {
      start = normalizeRangeEndpoint(start, "start");
      end = normalizeRangeEndpoint(end, "end");
      let resolvedType = type;
      if (resolvedType === undefined) {
        resolvedType = isIPv4(start) ? "ipv4" : "ipv6";
      } else {
        resolvedType = this._checkType(resolvedType);
      }
      if (resolvedType === "ipv4") {
        if (compareV4(start, end) > 0) {
          const e = new TypeError(
            'The value of "start" must be lower than "end" [ERR_INVALID_ARG_VALUE]',
          );
          e.code = "ERR_INVALID_ARG_VALUE";
          throw e;
        }
        this._v4Ranges.push([start, end]);
        this._rules.push(`Range: IPv4 ${start}-${end}`);
      } else {
        if (compareV6(start, end) > 0) {
          const e = new TypeError(
            'The value of "start" must be lower than "end" [ERR_INVALID_ARG_VALUE]',
          );
          e.code = "ERR_INVALID_ARG_VALUE";
          throw e;
        }
        this._v6Ranges.push([start, end]);
        this._rules.push(`Range: IPv6 ${start}-${end}`);
      }
    }
    addSubnet(net, prefix, type) {
      net = normalizeRangeEndpoint(net, "net");
      if (typeof prefix !== "number") {
        const e = new TypeError("Invalid prefix [ERR_INVALID_ARG_TYPE]");
        e.code = "ERR_INVALID_ARG_TYPE";
        throw e;
      }
      const resolvedType = resolveAddressType(
        net,
        type,
        (value) => this._checkType(value),
      );
      const maxPrefix = resolvedType === "ipv4" ? 32 : 128;
      if (!Number.isFinite(prefix) || prefix < 0 || prefix > maxPrefix) {
        const e = new TypeError(
          `Prefix must be between 0 and ${maxPrefix} [ERR_OUT_OF_RANGE]`,
        );
        e.code = "ERR_OUT_OF_RANGE";
        throw e;
      }
      if (resolvedType === "ipv4") {
        this._v4Subnets.push([net, prefix]);
        this._rules.push(`Subnet: IPv4 ${net}/${prefix}`);
      } else {
        this._v6Subnets.push([net, prefix]);
        this._rules.push(`Subnet: IPv6 ${net}/${prefix}`);
      }
    }
    check(address, type) {
      const { str, resolvedType, explicitType, inputKind } =
        resolveBlockListCheck(address, type, (value) => this._checkType(value));
      if (resolvedType === null) return false;
      return resolvedType === "ipv4"
        ? checkBlockListV4(this, str, explicitType, inputKind)
        : checkBlockListV6(this, str, explicitType, inputKind);
    }
  },
  SocketAddress: class SocketAddress {
    constructor(input) {
      this.address = input && input.address ? String(input.address) : "";
      this.family = input && input.family !== undefined
        ? input.family
        : undefined;
      this.flowlabel = (input && input.flowlabel) || 0;
      this.port = (input && input.port) || 0;
    }
  },
  createServer: (options, handler) => {
    if (typeof options === "function") {
      handler = options;
      options = {};
    }
    const server = new globalThis.__nodeEventEmitter();
    server.listening = false;
    server._handle = { close: () => {} };
    server._allowHalfOpen = options?.allowHalfOpen !== false;
    server.address = () =>
      server.listening
        ? { address: "127.0.0.1", family: "IPv4", port: 0 }
        : null;
    server.listen = (_port, callback) => {
      server.listening = true;
      __quenchNetServers.add(server);
      if (typeof callback === "function") {
        queueMicrotask(() => callback.call(server));
      }
      return server;
    };
    server.close = (callback) => {
      server.listening = false;
      __quenchNetServers.delete(server);
      if (typeof callback === "function") queueMicrotask(callback);
      return server;
    };
    server.unref = () => server;
    server._handler = handler;
    return server;
  },
};
globalThis.__quench_require_part_01 = (name, specifier) =>
  name === "net" ? __quenchNetModule : undefined;
