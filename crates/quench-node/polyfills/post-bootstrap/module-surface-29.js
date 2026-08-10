const __quenchClusterFallbacks = (result) => {
  result.isPrimary ??= true;
  result.isWorker ??= false;
  result.worker ??= undefined;
  result.workers ||= {};
  result.settings ||= {};
  result.fork ||= () => undefined;
  result.setupPrimary ||= () => undefined;
  result.disconnect ||= () => undefined;
  result.schedulingPolicy ??= 2;
  result.Worker ||= function Worker() {};
  return result;
};
const __quenchDomainFallbacks = (result) => {
  const makeDomain = () => ({
    add: () => undefined,
    remove: () => undefined,
    run: (callback) => callback?.(),
    enter: () => undefined,
    exit: () => undefined,
    bind: (callback) => callback,
    dispose: () => undefined
  });
  result.create ||= makeDomain;
  result.createDomain ||= result.create;
  if (result.active === undefined) result.active = null;
};
const __quenchHttp2Fallbacks = (result) => {
  for (const name of ["connect", "createServer", "createSecureServer"]) {
    result[name] ||= () => undefined;
  }
  for (const name of "Http2Server Http2SecureServer Http2Session Http2Stream".split(
    " "
  )) {
    result[name] ||= function Constructor() {};
  }
  result.constants ||= {};
  result.getDefaultSettings ||= () => ({});
  result.getPackedSettings ||= () => new Uint8Array();
  result.getUnpackedSettings ||= () => ({});
  result.sensitiveHeaders ||= () => [];
};
const __quenchSysFallbacks = (result) => {
  result.format ||= (...args) => args.join(" ");
  result.debug ||= () => undefined;
  result.inspect ||= (value) => String(value);
  result.log ||= () => undefined;
  result.inherits ||= (constructor, superConstructor) =>
    Object.setPrototypeOf(constructor.prototype, superConstructor.prototype);
  result.isArray ||= Array.isArray;
  result.isBoolean ||= (value) => typeof value === "boolean";
  result.isNull ||= (value) => value === null;
};
const __quenchApplyModuleSurface29 = (name, result) => {
  const normalized = String(name).replace(/^node:/, "");
  if (normalized === "cluster") __quenchClusterFallbacks(result);
  if (normalized === "domain") __quenchDomainFallbacks(result);
  if (normalized === "http2") __quenchHttp2Fallbacks(result);
  if (normalized === "sys") __quenchSysFallbacks(result);
  return result;
};
if (globalThis.require) {
  const originalRequire = globalThis.require;
  globalThis.require = (name) =>
    __quenchApplyModuleSurface29(name, originalRequire(name));
}
