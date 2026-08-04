globalThis.__quench_bootstrap_fragments.push(
  'const __quenchSourceMapsProcess = globalThis.process;\nconst __quenchSetSourceMapsEnabled = (value) => {\n  if (typeof value !== "boolean") {\n    const error = new TypeError("The \\\"val\\\" argument must be of type boolean [ERR_INVALID_ARG_TYPE]");\n    error.code = "ERR_INVALID_ARG_TYPE";\n    throw error;\n  }\n  __quenchSourceMapsProcess.__sourceMapsEnabled = value;\n};\nObject.defineProperty(__quenchSourceMapsProcess, "setSourceMapsEnabled", { get: () => __quenchSetSourceMapsEnabled, set: () => {}, configurable: true });\n'
);
