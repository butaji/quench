const __quenchOriginalRequireWithConsumers = globalThis.require;
const __quenchConsume = async (stream) => {
  const reader = stream?.getReader ? stream.getReader() : null;
  const chunks = [];
  if (reader) {
    for (;;) {
      const item = await reader.read();
      if (item.done) break;
      chunks.push(item.value);
    }
  } else if (stream != null) {
    await new Promise((resolve, reject) => {
      stream.on("data", (chunk) => chunks.push(chunk));
      stream.once("end", resolve);
      stream.once("error", reject);
    });
  }
  return chunks;
};
const __quenchConsumerBinaryChunk = (chunk) => {
  if (
    typeof chunk === "string" ||
    ArrayBuffer.isView(chunk) ||
    chunk instanceof ArrayBuffer ||
    chunk instanceof SharedArrayBuffer
  ) {
    return Buffer.from(chunk);
  }
  return Buffer.from(String(chunk));
};
const __quenchConsumerTextChunk = (chunk) => {
  if (
    typeof chunk === "string" ||
    ArrayBuffer.isView(chunk) ||
    chunk instanceof ArrayBuffer ||
    chunk instanceof SharedArrayBuffer
  ) {
    return Buffer.from(chunk);
  }
  throw Object.assign(new TypeError('The "chunk" argument must be of type string or an instance of Buffer or Uint8Array'), { code: "ERR_INVALID_ARG_TYPE" });
};
const __quenchStreamConsumers = {
  buffer: async (stream) =>
    Buffer.concat(
      (await __quenchConsume(stream)).map(__quenchConsumerBinaryChunk),
    ),
  arrayBuffer: async (stream) =>
    (await __quenchStreamConsumers.buffer(stream)).buffer,
  text: async (stream) =>
    Buffer.concat(
      (await __quenchConsume(stream)).map(__quenchConsumerTextChunk),
    ).toString(),
  json: async (stream) =>
    JSON.parse(await __quenchStreamConsumers.text(stream)),
  bytes: async (stream) =>
    new Uint8Array(await __quenchStreamConsumers.buffer(stream)),
  blob: async (stream) =>
    new Blob([await __quenchStreamConsumers.buffer(stream)]),
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "stream/consumers") {
    return __quenchStreamConsumers;
  }
  return __quenchOriginalRequireWithConsumers(specifier);
};
