//! Promise-based consumers for Web ReadableStreams.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const FACTORY: &str = quench_js_check::checked_js!(
    r#"(Buffer, Decoder) => {
  async function bytes(stream) {
    if (stream === null || typeof stream?.getReader !== "function") {
      throw new TypeError("The \"stream\" argument must be a ReadableStream");
    }
    const reader = stream.getReader();
    const chunks = [];
    let size = 0;
    try {
      for (;;) {
        const item = await reader.read();
        if (item.done) break;
        let chunk = item.value;
        if (typeof chunk === "string") chunk = new TextEncoder().encode(chunk);
        else if (ArrayBuffer.isView(chunk)) chunk = new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
        else if (chunk instanceof ArrayBuffer) chunk = new Uint8Array(chunk);
        else throw new TypeError("The stream yielded a non-byte chunk");
        chunks.push(chunk);
        size += chunk.byteLength;
      }
    } catch (error) {
      try { await reader.cancel(error); } catch {}
      throw error;
    } finally {
      reader.releaseLock();
    }
    const output = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) { output.set(chunk, offset); offset += chunk.byteLength; }
    return output;
  }
  async function arrayBuffer(stream) { return (await bytes(stream)).buffer; }
  async function buffer(stream) { return Buffer.from(await bytes(stream)); }
  async function text(stream) { return new Decoder().decode(await bytes(stream)); }
  async function json(stream) { return JSON.parse(await text(stream)); }
  async function blob(stream) { return new Blob([await bytes(stream)]); }
  return { arrayBuffer, blob, buffer, json, text };
}"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let global = context.global_root()?;
    let buffer_key = context.string_rooted("Buffer");
    let buffer = context.get_property_rooted(global, buffer_key)?;
    let decoder_key = context.string_rooted("TextDecoder");
    let decoder = context.get_property_rooted(global, decoder_key)?;
    let factory = context.evaluate_script_rooted(
        FACTORY,
        "node:stream/consumers/shared.js",
    )?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[buffer, decoder])
}
