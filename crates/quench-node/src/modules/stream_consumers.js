// Stream consumers are one composed reduction over either the WHATWG reader
// protocol or the Node EventEmitter protocol. The public helpers below share
// that one binary/text chunk normalization fact.
(function () {
  const consume = (stream) => {
    const reader = stream?.getReader ? stream.getReader() : null;
    if (reader) {
      const chunks = [];
      const read = () => reader.read().then((step) => {
        if (step.done) return chunks;
        chunks.push(step.value);
        return read();
      });
      return read();
    }
    return new Promise((resolve, reject) => {
      const chunks = [];
      if (!stream || typeof stream.on !== "function") {
        reject(Object.assign(new TypeError("The \"stream\" argument must be an instance of Stream"), {
          code: "ERR_INVALID_ARG_TYPE"
        }));
        return;
      }
      stream.on("data", (chunk) => chunks.push(chunk));
      stream.once("end", () => resolve(chunks));
      stream.once("error", reject);
    });
  };
  const binaryChunk = (chunk) => {
    if (typeof chunk === "string" || ArrayBuffer.isView(chunk) ||
        chunk instanceof ArrayBuffer || chunk instanceof SharedArrayBuffer) {
      return Buffer.from(chunk);
    }
    return Buffer.from(String(chunk));
  };
  const textChunk = (chunk) => {
    if (typeof chunk === "string" || ArrayBuffer.isView(chunk) ||
        chunk instanceof ArrayBuffer || chunk instanceof SharedArrayBuffer) {
      return Buffer.from(chunk);
    }
    throw Object.assign(new TypeError("The \"chunk\" argument must be of type string or an instance of Buffer or Uint8Array"), {
      code: "ERR_INVALID_ARG_TYPE"
    });
  };
  const buffer = async (stream) =>
    Buffer.concat((await consume(stream)).map(binaryChunk));
  const text = async (stream) =>
    Buffer.concat((await consume(stream)).map(textChunk)).toString();
  return {
    buffer,
    arrayBuffer: async (stream) => (await buffer(stream)).buffer,
    text,
    json: async (stream) => JSON.parse(await text(stream)),
    bytes: async (stream) => new Uint8Array(await buffer(stream)),
    blob: async (stream) => new Blob([await buffer(stream)])
  };
})()
