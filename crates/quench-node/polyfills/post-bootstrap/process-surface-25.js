{
  if (globalThis.process) {
    const originalBinding = globalThis.process.binding;
    globalThis.process.binding = (name) => {
      if (name === "util" && globalThis.__nodeUtil?.types) {
        const types = globalThis.__nodeUtil.types;
        return {
          isAnyArrayBuffer: types.isAnyArrayBuffer,
          isArrayBuffer: types.isArrayBuffer,
          isArrayBufferView: types.isArrayBufferView,
          isAsyncFunction: types.isAsyncFunction,
          isDataView: types.isDataView,
          isDate: types.isDate,
          isExternal: types.isExternal,
          isMap: types.isMap,
          isMapIterator: types.isMapIterator,
          isNativeError: types.isNativeError,
          isPromise: types.isPromise,
          isRegExp: types.isRegExp,
          isSet: types.isSet,
          isSetIterator: types.isSetIterator,
          isTypedArray: types.isTypedArray,
          isUint8Array: types.isUint8Array
        };
      }
      return originalBinding(name);
    };
    globalThis.process._linkedBinding ||= () => ({});
    globalThis.process.dlopen ||= () => undefined;
  }
}
