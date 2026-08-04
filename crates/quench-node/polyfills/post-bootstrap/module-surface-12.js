const __quenchSetFallbacks = (result, names, fallback) => {
  for (const name of names) result[name] ||= fallback;
};
const __quenchWorkerThreadFallbacks = (result) => {
  __quenchSetFallbacks(
    result,
    [
      "Worker",
      "MessageChannel",
      "MessagePort",
      "BroadcastChannel",
      "receiveMessageOnPort",
      "markAsUncloneable",
      "setEnvironmentData",
      "getEnvironmentData",
      "markAsUntransferable",
      "isMarkedAsUncloneable",
      "moveMessagePortToContext"
    ],
    () => undefined
  );
  result.parentPort ??= null;
  result.workerData ??= undefined;
  result.threadId ??= 0;
};
const __quenchFsModuleFallbacks = (result) => {
  __quenchSetFallbacks(
    result,
    ["glob", "cp", "cpSync", "watch", "watchFile", "unwatchFile"],
    () => undefined
  );
  __quenchSetFallbacks(
    result,
    [
      "FSWatcher",
      "StatWatcher",
      "opendir",
      "opendirSync",
      "Dir",
      "Dirent",
      "ReadStream",
      "WriteStream"
    ],
    function Constructor() {}
  );
  result.promises ||= {};
  result.promises.glob ||= async function* () {};
  result.promises.cp ||= async () => undefined;
  result.promises.opendir ||= async () => undefined;
};
const __quenchZlibFallbacks = (result) =>
  __quenchSetFallbacks(
    result,
    [
      "deflateRaw",
      "deflateRawSync",
      "inflateRaw",
      "inflateRawSync",
      "brotliCompress",
      "brotliCompressSync",
      "brotliDecompress",
      "brotliDecompressSync",
      "unzip",
      "unzipSync"
    ],
    () => undefined
  );
const __quenchApplyModuleSurface12 = (name, result) => {
  const normalized = String(name).replace(/^node:/, "");
  if (normalized === "worker_threads") __quenchWorkerThreadFallbacks(result);
  if (normalized === "fs") __quenchFsModuleFallbacks(result);
  if (normalized === "zlib") __quenchZlibFallbacks(result);
  return result;
};
if (globalThis.require) {
  const originalRequire = globalThis.require;
  globalThis.require = (name) =>
    __quenchApplyModuleSurface12(name, originalRequire(name));
}
