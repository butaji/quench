const { types } = require("util");
const { internalBinding } = require("internal/test/binding");

const cases = [
  [new (internalBinding("js_stream").JSStream)()._externalStream, "isExternal"],
  [new Date(), "isDate"],
  [
    (function () {
      return arguments;
    })(),
    "isArgumentsObject",
  ],
  [new Boolean(), "isBooleanObject"],
  [new Number(), "isNumberObject"],
  [new String(), "isStringObject"],
  [Object(Symbol()), "isSymbolObject"],
  [Object(1n), "isBigIntObject"],
  [new Error(), "isNativeError"],
  [/x/, "isRegExp"],
  [async function () {}, "isAsyncFunction"],
  [function* () {}, "isGeneratorFunction"],
  [(function* () {})(), "isGeneratorObject"],
  [Promise.resolve(), "isPromise"],
  [new Map(), "isMap"],
  [new Set(), "isSet"],
  [new Map().keys(), "isMapIterator"],
  [new Set().keys(), "isSetIterator"],
  [new WeakMap(), "isWeakMap"],
  [new WeakSet(), "isWeakSet"],
  [new ArrayBuffer(), "isArrayBuffer"],
  [new SharedArrayBuffer(), "isSharedArrayBuffer"],
  [new Uint8Array(), "isUint8Array"],
  [
    Object.defineProperty(new Uint8Array(), Symbol.toStringTag, {
      value: "foo",
    }),
    "isUint8Array",
  ],
  [new Uint8ClampedArray(), "isUint8ClampedArray"],
  [new Uint16Array(), "isUint16Array"],
  [new Uint32Array(), "isUint32Array"],
  [new Int8Array(), "isInt8Array"],
  [new Int16Array(), "isInt16Array"],
  [new Int32Array(), "isInt32Array"],
  [new Float16Array(), "isFloat16Array"],
  [new Float32Array(), "isFloat32Array"],
  [new Float64Array(), "isFloat64Array"],
  [new BigInt64Array(), "isBigInt64Array"],
  [new BigUint64Array(), "isBigUint64Array"],
  [new DataView(new ArrayBuffer()), "isDataView"],
  [new Proxy({}, {}), "isProxy"],
];

for (const [value, method] of cases) {
  if (!types[method](value)) throw new Error(`failed ${method}`);
  for (const key of Object.keys(types)) {
    if (
      ((types.isArrayBufferView(value) || types.isAnyArrayBuffer(value)) &&
        key.includes("Array")) ||
      key === "isBoxedPrimitive"
    ) {
      continue;
    }
    if (types[key](value) !== (key === method)) {
      throw new Error(`${method}: ${key}=${types[key](value)}`);
    }
  }
}
