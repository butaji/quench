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
const __quenchStreamConsumers = {
  buffer: async (stream) =>
    Buffer.concat(
      (await __quenchConsume(stream)).map((chunk) => Buffer.from(chunk))
    ),
  arrayBuffer: async (stream) =>
    (await __quenchStreamConsumers.buffer(stream)).buffer,
  text: async (stream) =>
    (await __quenchStreamConsumers.buffer(stream)).toString(),
  json: async (stream) =>
    JSON.parse(await __quenchStreamConsumers.text(stream)),
  bytes: async (stream) =>
    new Uint8Array(await __quenchStreamConsumers.buffer(stream)),
  blob: async (stream) =>
    new Blob([await __quenchStreamConsumers.buffer(stream)])
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "stream/consumers") {
    return __quenchStreamConsumers;
  }
  return __quenchOriginalRequireWithConsumers(specifier);
};
