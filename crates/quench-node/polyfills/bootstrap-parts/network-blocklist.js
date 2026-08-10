const __quenchNetBlockList = class BlockList {
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
};
