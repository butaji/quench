//! Polyfill: `network-head`

pub const JS: &str = r#"const isIPv4Part = (part) => {
  if (!/^\d+$/.test(part)) return false;
  const n = Number(part);
  if (n < 0 || n > 255) return false;
  if (part.length > 1 && part.startsWith("0")) return false;
  return part.length <= 3;
};
const __quenchNetServers = new Set();
let __quenchNextEphemeralPort = 40000;
const __quenchBoundPorts = new Set();
const __quenchBoundPaths = new Set();
const __quenchNativeSockets = new Set();
const __quenchDeliverPendingWrites = (socket, writes) => {
  for (const chunk of writes) {
    const delivered = NodeBuffer.from(chunk);
    if (socket._paused || socket.listenerCount("data") === 0) {
      socket._pendingData.push(delivered);
    } else {
      queueMicrotask(() => socket.emit("data", delivered));
    }
  }
};
const __quenchSetTypeOfService = (socket, value) => {
  if (typeof value !== "number" || Number.isNaN(value)) {
    throw Object.assign(new TypeError('The "tos" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (!Number.isInteger(value) || value < 0 || value > 255) {
    throw Object.assign(new RangeError('The value of "tos" is out of range'), { code: "ERR_OUT_OF_RANGE" });
  }
  socket._typeOfService = value;
  return socket;
};
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
  isHead
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
const countIPv6Groups = (headGroups, tailGroups, hasDoubleColon) =>
  countIPv6GroupList(headGroups, hasDoubleColon, true) +
  countIPv6GroupList(tailGroups, hasDoubleColon, false);
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
  if (
    (input.startsWith(":") && !input.startsWith("::")) ||
    (input.endsWith(":") && !input.endsWith("::"))
  ) {
    return false;
  }
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
      explicit: address.family !== undefined
    };
  }
  throw Object.assign(new TypeError(`Invalid ${label}`), { code: "ERR_INVALID_ARG_TYPE" });
};
const normalizeRangeEndpoint = (value, label) => {
  if (typeof value === "string") return value;
  if (value && typeof value.address === "string") return value.address;
  throw Object.assign(new TypeError(`Invalid ${label}`), { code: "ERR_INVALID_ARG_TYPE" });
};
const resolveAddressType = (value, type, checkType) => {
  if (type !== undefined) return checkType(type);
  const resolved = isIPv4(value) ? "ipv4" : isIPv6(value) ? "ipv6" : null;
  if (resolved) return resolved;
  throw Object.assign(new TypeError("Invalid address"), { code: "ERR_INVALID_ARG_VALUE" });
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
  const inputKind =
    address &&
    typeof address !== "string" &&
    typeof address.address === "string"
      ? "socket"
      : "string";
  return { str, resolvedType, explicitType, inputKind };
};
"#;
