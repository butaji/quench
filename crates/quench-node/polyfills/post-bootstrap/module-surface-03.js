const __quenchAddStreamAliases = (result) => {
  result.Stream ||= result.Readable;
  result.Writable ||= result.Readable;
  result.Duplex ||= result.Transform;
};
const __quenchAddStreamWebCompat = (result) => {
  for (const name of ["Readable", "Writable", "Duplex"]) {
    result[name].toWeb ||= () => ({});
    result[name].fromWeb ||= (value) => value;
  }
};
const __quenchAddStreamDefaults = (result) => {
  result.pipeline ||= () => undefined;
  result.finished ||= () => undefined;
  result.addAbortSignal ||= () => undefined;
  result.compose ||= (stream) => stream;
  result.setDefaultHighWaterMark ||= () => 16384;
  result.getDefaultHighWaterMark ||= () => 16384;
};
const __quenchAddStreamCompat = (result) => {
  __quenchAddStreamAliases(result);
  __quenchAddStreamWebCompat(result);
  __quenchAddStreamDefaults(result);
  return result;
};
if (globalThis.require) {
  const originalRequire = globalThis.require;
  globalThis.require = (name) => {
    const result = originalRequire(name);
    if (String(name).replace(/^node:/, "") === "stream")
      return __quenchAddStreamCompat(result);
    return result;
  };
}
