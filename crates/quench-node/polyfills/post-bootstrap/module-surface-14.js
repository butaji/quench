const __quenchDnsFallbacks = (result) => {
  result = Object.assign({}, result);
  result.resolve ||= (hostname, callback) => callback?.(null, []);
  result.resolve4 ||= result.resolve;
  result.resolve6 ||= result.resolve;
  result.reverse ||= result.resolve;
  result.getDefaultResultOrder ||= () => "verbatim";
  result.setDefaultResultOrder ||= () => undefined;
  result.promises ||= {};
  for (const method of ["lookup", "resolve", "resolve4", "resolve6", "reverse"])
    result.promises[method] ||= async () => [];
  return result;
};
if (globalThis.require) {
  const originalRequire = globalThis.require;
  globalThis.require = (name) => {
    const result = originalRequire(name);
    if (String(name).replace(/^node:/, "") === "dns")
      return __quenchDnsFallbacks(result);
    return result;
  };
}
