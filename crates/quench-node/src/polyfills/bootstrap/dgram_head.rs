//! Polyfill: `dgram-head`

pub const JS: &str = quench_js_check::checked_js!(r#"/* eslint-disable max-lines-per-function, complexity */
const __quenchOriginalRequireWithDgram = globalThis.require;
const NodeBuffer = globalThis.Buffer;
const __quenchDgramInternalsKey = Symbol.for("quench.dgram.internals");
const __quenchDgramInternals = globalThis[__quenchDgramInternalsKey] || {
  stateSymbol: Symbol.for("quench.dgram.state"),
  boundPorts: new Set(),
  closedPorts: new Set(),
  sockets: new Set(),
};
globalThis[__quenchDgramInternalsKey] = __quenchDgramInternals;
var __quenchDgramStateSymbol = __quenchDgramInternals.stateSymbol;
var __quenchDgramBoundPorts = __quenchDgramInternals.boundPorts;
var __quenchDgramClosedPorts = __quenchDgramInternals.closedPorts;
var __quenchDgramSockets = __quenchDgramInternals.sockets;
var __quenchDgramNextPort = 40000;
Object.defineProperties(globalThis, {
  __quenchDgramActiveFds: { value: new Set(), configurable: true },
  __quenchDgramUdpFds: { value: new Set(), configurable: true },
  __quenchDgramUdpHandleInfo: { value: new Map(), configurable: true },
});
const __quenchDgramTypeDetail = (value) => {
  if (value === null) return " Received null";
  if (value === undefined) return " Received undefined";
  if (typeof value === "string") return ` Received type string ('${value}')`;
  if (typeof value === "number") return ` Received type number (${value})`;
  if (typeof value === "boolean") return ` Received type boolean (${value})`;
  if (typeof value === "bigint") return ` Received type bigint (${value}n)`;
  if (typeof value === "symbol") {
    return ` Received type symbol (${String(value)})`;
  }
  if (Array.isArray(value)) return " Received an instance of Array";
  return ` Received an instance of ${value?.constructor?.name || "Object"}`;
};
const __quenchDgramBufferError = (type, code, message) => {
  const syscall = `uv_${type}_buffer_size`;
  const error = new Error(
    `Could not get or set buffer size: ${syscall} returned ${code} (${message})`
  );
  error.name = "SystemError";
  error.code = "ERR_SOCKET_BUFFER_SIZE";
  error.info = { errno: undefined, code, message, syscall };
  let errorErrno;
  Object.defineProperty(error, "errno", {
    enumerable: true,
    get: () => errorErrno,
    set: (value) => {
      errorErrno = value;
    }
  });
  let errorSyscall = syscall;
  Object.defineProperty(error, "syscall", {
    enumerable: true,
    get: () => errorSyscall,
    set: (value) => {
      errorSyscall = value;
    }
  });
  return error;
};
"#);
