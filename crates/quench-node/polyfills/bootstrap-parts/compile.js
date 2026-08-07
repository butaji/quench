const __quenchVmCompileExtensions = (options) => [
  ...(options?.parsingContext ? [options.parsingContext] : []),
  ...(options?.contextExtensions || []),
];
const __quenchVmCompilePrefix = (extensions) => {
  const names = [
    ...new Set(extensions.flatMap((extension) => Object.keys(extension))),
  ];
  return names
    .map(
      (name) =>
        `const ${name} = __extensions.reduce((value, item) => value ?? item?.[${
          JSON.stringify(name)
        }], undefined);`,
    )
    .join("\n");
};
const __quenchVmInvokeCompiled = (compiled, extensions, args, options) => {
  try {
    return compiled(extensions, ...args);
  } catch (error) {
    const line = (options?.lineOffset || 0) + 1;
    const column = error.name === "Error"
      ? (options?.columnOffset || 0) + 7
      : 1;
    error.stack =
      `${error.name}: ${error.message}\n    at <anonymous>:${line}:${column}`;
    throw error;
  }
};
const __quenchVmCreateCompiledFunction = (
  compiled,
  extensions,
  options,
  code,
  params,
) => {
  const fn = extensions.length
    ? (...args) => __quenchVmInvokeCompiled(compiled, extensions, args, options)
    : (...args) => __quenchVmInvokeCompiled(compiled, [], args, options);
  if (!params.length) fn.toString = () => `function () {\n${code}\n}`;
  if (options?.produceCachedData) {
    fn.cachedDataProduced = true;
    fn.cachedData = NodeBuffer.from(code);
  }
  if (options?.cachedData) {
    fn.cachedDataRejected = !__quenchVmCacheMatches(options.cachedData, code);
  }
  return fn;
};
const __quenchVmCompileFunction = (code, params = [], options) => {
  if (typeof code !== "string") {
    __quenchVmTypeError(
      'The "code" argument must be of type string. Received undefined',
    );
  }
  if (!Array.isArray(params)) {
    __quenchVmTypeError(
      `The "params" argument must be an instance of Array.${
        __quenchVmInvalidTypeSuffix(params)
      }`,
    );
  }
  __quenchVmValidateCompileOptions(options);
  if (code.trimStart().startsWith("});")) {
    throw new SyntaxError("Unexpected token '}'");
  }
  const extensions = __quenchVmCompileExtensions(options);
  const prefix = __quenchVmCompilePrefix(extensions);
  const compiled = Function("__extensions", ...params, `${prefix}\n${code}`);
  return __quenchVmCreateCompiledFunction(
    compiled,
    extensions,
    options,
    code,
    params,
  );
};
