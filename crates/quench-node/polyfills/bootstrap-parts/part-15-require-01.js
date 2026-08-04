globalThis.__quench_require_part_01 = (name, specifier) => {
  if (name === "net") {
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
      for (const part of parts) {
        if (!/^\d+$/.test(part)) return false;
        const n = Number(part);
        if (n < 0 || n > 255) return false;
        if (part.length > 1 && part.startsWith("0")) return false;
        if (part.length > 3) return false;
      }
      return true;
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
      if (input.length === 0) return false;
      let address = input;
      if (address.includes("%")) {
        const percentIndex = address.indexOf("%");
        if (percentIndex === address.length - 1) return false;
        if (address.indexOf("%", percentIndex + 1) !== -1) return false;
        const zone = address.slice(percentIndex + 1);
        if (
          zone.includes(":") ||
          zone.includes("%") ||
          zone.includes("@") ||
          zone.length === 0 ||
          !/^[0-9A-Za-z._-]+$/.test(zone)
        )
          return false;
        address = address.slice(0, percentIndex);
        if (address.length === 0) return false;
      }
      if (address === "::") return true;
      const doubleColonIndex = address.indexOf("::");
      if (
        doubleColonIndex !== -1 &&
        address.indexOf("::", doubleColonIndex + 1) !== -1
      )
        return false;
      const hasDoubleColon = doubleColonIndex !== -1;
      const head = hasDoubleColon
        ? address.slice(0, doubleColonIndex)
        : address;
      const tail = hasDoubleColon ? address.slice(doubleColonIndex + 2) : "";
      const headGroups = head === "" ? [] : head.split(":");
      const tailGroups = tail === "" ? [] : tail.split(":");
      let expanded = 0;
      const validateGroup = (group) => {
        if (group.includes(".")) {
          const parts = group.split(".");
          if (parts.length !== 4) return false;
          for (const part of parts) {
            if (!/^\d+$/.test(part)) return false;
            const n = Number(part);
            if (n < 0 || n > 255) return false;
            if (part.length > 1 && part.startsWith("0")) return false;
          }
          expanded += 2;
        } else {
          if (group.length === 0 || group.length > 4) return false;
          if (!/^[0-9a-fA-F]+$/.test(group)) return false;
          expanded++;
        }
        return true;
      };
      for (let i = 0; i < headGroups.length; i++) {
        const group = headGroups[i];
        if (hasDoubleColon && group.includes(".")) return false;
        if (!hasDoubleColon && group.includes(".") && i < headGroups.length - 1)
          return false;
        if (!validateGroup(group)) return false;
      }
      for (let i = 0; i < tailGroups.length; i++) {
        const group = tailGroups[i];
        if (i < tailGroups.length - 1 && group.includes(".")) return false;
        if (!validateGroup(group)) return false;
      }
      if (hasDoubleColon) {
        if (expanded > 7) return false;
      } else {
        if (expanded !== 8) return false;
      }
      return true;
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
      if (typeof address === "string")
        return { value: address, explicit: false };
      if (address && typeof address.address === "string")
        return {
          value: address.address,
          explicit: address.family !== undefined
        };
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
    return {
      isIP,
      isIPv4,
      isIPv6,
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
              `Invalid type '${type}' [ERR_INVALID_ARG_VALUE]`
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
          let resolvedType = type;
          if (resolvedType === undefined) {
            resolvedType = isIPv4(str) ? "ipv4" : isIPv6(str) ? "ipv6" : null;
            if (resolvedType === null) {
              const e = new TypeError("Invalid address");
              e.code = "ERR_INVALID_ARG_VALUE";
              throw e;
            }
          } else {
            resolvedType = this._checkType(resolvedType);
          }
          if (resolvedType === "ipv4") {
            const existing = this._v4.get(str);
            this._v4.set(str, {
              explicit: (existing && existing.explicit) || explicit
            });
            this._rules.push(`Address: IPv4 ${str}`);
          } else {
            const existing = this._v6.get(str);
            this._v6.set(str, {
              explicit: (existing && existing.explicit) || explicit
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
                'The value of "start" must be lower than "end" [ERR_INVALID_ARG_VALUE]'
              );
              e.code = "ERR_INVALID_ARG_VALUE";
              throw e;
            }
            this._v4Ranges.push([start, end]);
            this._rules.push(`Range: IPv4 ${start}-${end}`);
          } else {
            if (compareV6(start, end) > 0) {
              const e = new TypeError(
                'The value of "start" must be lower than "end" [ERR_INVALID_ARG_VALUE]'
              );
              e.code = "ERR_INVALID_ARG_VALUE";
              throw e;
            }
            this._v6Ranges.push([start, end]);
            this._rules.push(`Range: IPv6 ${start}-${end}`);
          }
        }
        addSubnet(net, prefix, type) {
          if (typeof net === "string") {
            // keep
          } else if (net && typeof net.address === "string") {
            net = net.address;
          } else {
            const e = new TypeError("Invalid net [ERR_INVALID_ARG_TYPE]");
            e.code = "ERR_INVALID_ARG_TYPE";
            throw e;
          }
          if (typeof prefix !== "number") {
            const e = new TypeError("Invalid prefix [ERR_INVALID_ARG_TYPE]");
            e.code = "ERR_INVALID_ARG_TYPE";
            throw e;
          }
          let resolvedType = type;
          if (resolvedType === undefined) {
            resolvedType = isIPv4(net) ? "ipv4" : "ipv6";
          } else {
            resolvedType = this._checkType(resolvedType);
          }
          const maxPrefix = resolvedType === "ipv4" ? 32 : 128;
          if (!Number.isFinite(prefix) || prefix < 0 || prefix > maxPrefix) {
            const e = new TypeError(
              `Prefix must be between 0 and ${maxPrefix} [ERR_OUT_OF_RANGE]`
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
          let str;
          if (typeof address === "string") str = address;
          else if (address && typeof address.address === "string")
            str = address.address;
          else {
            const e = new TypeError("Invalid address [ERR_INVALID_ARG_TYPE]");
            e.code = "ERR_INVALID_ARG_TYPE";
            throw e;
          }
          let resolvedType = type;
          if (typeof resolvedType === "string")
            resolvedType = resolvedType.toLowerCase();
          const explicitType = resolvedType !== undefined;
          if (resolvedType === undefined) {
            if (isIPv4(str)) resolvedType = "ipv4";
            else if (isIPv6(str)) resolvedType = "ipv6";
            else return false;
          } else {
            this._checkType(resolvedType);
          }
          let inputKind = "string";
          if (
            address &&
            typeof address !== "string" &&
            typeof address.address === "string"
          ) {
            inputKind = "socket";
          }
          if (resolvedType === "ipv4") {
            if (this._v4.has(str)) {
              const entry = this._v4.get(str);
              if (
                explicitType ||
                inputKind === "socket" ||
                (inputKind === "string" && !entry.explicit)
              )
                return true;
            }
            for (const [s, e] of this._v4Ranges) {
              if (compareV4(str, s) >= 0 && compareV4(str, e) <= 0) return true;
            }
            for (const [net, prefix] of this._v4Subnets) {
              if (matchSubnetV4(str, net, prefix)) return true;
            }
            const mapped = "::ffff:" + str;
            if (this._v6.has(mapped)) return true;
          } else {
            if (this._v6.has(str)) {
              const entry = this._v6.get(str);
              if (
                explicitType ||
                inputKind === "socket" ||
                (inputKind === "string" && !entry.explicit)
              )
                return true;
            }
            for (const [s, e] of this._v6Ranges) {
              if (compareV6(str, s) >= 0 && compareV6(str, e) <= 0) return true;
            }
            for (const [net, prefix] of this._v6Subnets) {
              if (matchSubnetV6(str, net, prefix)) return true;
            }
            const dc = str.indexOf("::ffff:");
            if (dc !== -1) {
              const tail = str.slice(dc + 7);
              let v4 = null;
              if (tail.indexOf(":") === -1) {
                v4 = tail;
              } else {
                const parts = tail.split(":");
                if (
                  parts.length === 2 &&
                  /^[0-9a-fA-F]+$/.test(parts[0]) &&
                  /^[0-9a-fA-F]+$/.test(parts[1])
                ) {
                  const a = parseInt(parts[0], 16);
                  const b = parseInt(parts[1], 16);
                  v4 = `${(a >> 8) & 0xff}.${a & 0xff}.${(b >> 8) & 0xff}.${b & 0xff}`;
                }
              }
              if (v4 !== null) {
                if (this._v4.has(v4)) return true;
                for (const [s, e] of this._v4Ranges) {
                  if (compareV4(v4, s) >= 0 && compareV4(v4, e) <= 0)
                    return true;
                }
                for (const [net, prefix] of this._v4Subnets) {
                  if (matchSubnetV4(v4, net, prefix)) return true;
                }
              }
            }
          }
          return false;
        }
      },
      SocketAddress: class SocketAddress {
        constructor(input) {
          this.address = input && input.address ? String(input.address) : "";
          this.family =
            input && input.family !== undefined ? input.family : undefined;
          this.flowlabel = (input && input.flowlabel) || 0;
          this.port = (input && input.port) || 0;
        }
      },
      createServer: (handler) => {
        const server = new globalThis.__nodeEventEmitter();
        server._handle = { close: () => {} };
        server.listen = (_port, callback) => {
          if (typeof callback === "function") queueMicrotask(callback);
          return server;
        };
        server.close = (callback) => {
          if (typeof callback === "function") queueMicrotask(callback);
          return server;
        };
        server.unref = () => server;
        server._handler = handler;
        return server;
      }
    };
  }
};
