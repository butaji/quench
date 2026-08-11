//! Polyfill: `types`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithUtilTypes = globalThis.require;
const __quenchUtilTypes = {
  isAnyArrayBuffer: (value) =>
    value instanceof ArrayBuffer ||
    (typeof SharedArrayBuffer !== "undefined" &&
      value instanceof SharedArrayBuffer),
  isArrayBuffer: (value) => value instanceof ArrayBuffer,
  isArrayBufferView: (value) => ArrayBuffer.isView(value),
  isArgumentsObject: (value) =>
    Object.prototype.toString.call(value) === "[object Arguments]",
  isDataView: (value) => value instanceof DataView,
  isDate: (value) => value instanceof Date,
  isMap: (value) => value instanceof Map,
  isPromise: (value) => value instanceof Promise,
  isRegExp: (value) => value instanceof RegExp,
  isSet: (value) => value instanceof Set,
  isTypedArray: (value) =>
    ArrayBuffer.isView(value) && !(value instanceof DataView),
  isUint8Array: (value) => value instanceof Uint8Array,
  isUint8ClampedArray: (value) => value instanceof Uint8ClampedArray,
  isUint16Array: (value) => value instanceof Uint16Array,
  isUint32Array: (value) => value instanceof Uint32Array,
  isFloat32Array: (value) => value instanceof Float32Array,
  isFloat64Array: (value) => value instanceof Float64Array,
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "util/types") {
    return __quenchUtilTypes;
  }
  return __quenchOriginalRequireWithUtilTypes(specifier);
};
"#);
