const __quenchVmFormatStack = (
  error,
  filename,
  lineOffset,
  columnOffset,
  code
) =>
  code
    ? `${filename}:${lineOffset + 1}\n${code}\n ^\n\n${error.name}: ${error.message}\nat ${filename}:${lineOffset + 1}:${columnOffset + 7}`
    : `${filename}:${lineOffset + 1}:${columnOffset + 8}\n ^\n${error.stack}`;
const __quenchVmRunInNewContext = (code, sandbox, options) => {
  if (!__quenchVmIsObject(sandbox))
    __quenchVmTypeError("The context argument must be an object");
  const keys = Object.getOwnPropertyNames(sandbox);
  const preservesHostGlobals = keys.some(
    (key) => typeof sandbox[key] === "function"
  );
  const original = new Map(
    Object.getOwnPropertyNames(globalThis).map((key) => [
      key,
      Object.getOwnPropertyDescriptor(globalThis, key)
    ])
  );
  try {
    for (const key of keys) globalThis[key] = sandbox[key];
    const result = __quenchVmEvaluateContext(code, sandbox, options, {
      keys,
      originalGlobalKeys: new Set(original.keys()),
      formatCode: true
    });
    return result;
  } finally {
    if (!preservesHostGlobals) {
      for (const key of Object.getOwnPropertyNames(globalThis))
        if (!original.has(key)) Reflect.deleteProperty(globalThis, key);
      for (const [key, descriptor] of original)
        Object.defineProperty(globalThis, key, descriptor);
    } else {
      for (const key of keys) {
        if (original.has(key))
          Object.defineProperty(globalThis, key, original.get(key));
        else Reflect.deleteProperty(globalThis, key);
      }
    }
  }
};
