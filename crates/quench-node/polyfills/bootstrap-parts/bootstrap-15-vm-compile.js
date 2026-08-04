const __quenchVmCompileExtensions = (options) => [
  ...(options?.parsingContext ? [options.parsingContext] : []),
  ...(options?.contextExtensions || [])
];
const __quenchVmCompilePrefix = (extensions) => {
  const names = [
    ...new Set(extensions.flatMap((extension) => Object.keys(extension)))
  ];
  return names
    .map(
      (name) =>
        `const ${name} = __extensions.reduce((value, item) => value ?? item?.[${JSON.stringify(name)}], undefined);`
    )
    .join("\n");
};
const __quenchVmCompileFunction = (code, params = [], options) => {
  if (typeof code !== "string")
    __quenchVmTypeError(
      'The "code" argument must be of type string. Received undefined'
    );
  if (!Array.isArray(params))
    __quenchVmTypeError(
      `The "params" argument must be an instance of Array.${__quenchVmInvalidTypeSuffix(params)}`
    );
  __quenchVmValidateCompileOptions(options);
  if (code.trimStart().startsWith("});"))
    throw new SyntaxError("Unexpected token '}'");
  const extensions = __quenchVmCompileExtensions(options);
  const prefix = __quenchVmCompilePrefix(extensions);
  const compiled = Function("__extensions", ...params, `${prefix}\n${code}`);
  const fn = extensions.length
    ? (...args) => compiled(extensions, ...args)
    : (...args) => compiled(undefined, ...args);
  if (!params.length) fn.toString = () => `function () {\n${code}\n}`;
  return fn;
};
