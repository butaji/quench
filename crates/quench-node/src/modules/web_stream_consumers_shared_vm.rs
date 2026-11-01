//! Promise-based consumers for Web ReadableStreams.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const FACTORY: &str = quench_js_check::checked_js!(
    r#"(Buffer, Decoder) => {
  async function bytes(stream) { return new Uint8Array(await (await blob(stream)).arrayBuffer()); }
  async function blob(stream) {
    const reader = typeof stream?.getReader === "function" ? stream.getReader() : undefined;
    const iterator = reader ? { next: () => reader.read() } : (typeof stream?.[Symbol.asyncIterator] === "function"
      ? stream[Symbol.asyncIterator]()
      : typeof stream?.[Symbol.iterator] === "function" ? stream[Symbol.iterator]() : undefined);
    if (iterator === undefined) {
      const error = new TypeError('The "stream" argument must be an AsyncIterable, Iterable, or ReadableStream');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    const chunks = [];
    try {
      for (;;) {
        const item = await iterator.next();
        if (item.done) break;
        chunks.push(item.value);
      }
    } catch (error) {
      try { if (reader) await reader.cancel(error); else await iterator.return?.(); } catch {}
      throw error;
    } finally { reader?.releaseLock(); }
    return new Blob(chunks);
  }
  async function arrayBuffer(stream) { return (await blob(stream)).arrayBuffer(); }
  async function buffer(stream) { return Buffer.from(await arrayBuffer(stream)); }
  async function text(stream) {
    const reader = typeof stream?.getReader === "function" ? stream.getReader() : undefined;
    const iterator = reader ? { next: () => reader.read() } : (typeof stream?.[Symbol.asyncIterator] === "function"
      ? stream[Symbol.asyncIterator]()
      : typeof stream?.[Symbol.iterator] === "function" ? stream[Symbol.iterator]() : undefined);
    if (iterator === undefined) {
      const error = new TypeError('The "stream" argument must be an AsyncIterable, Iterable, or ReadableStream');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    const decoder = new Decoder();
    let result = '';
    try {
      for (;;) {
        const item = await iterator.next();
        if (item.done) break;
        if (typeof item.value === 'string') result += item.value;
        else if (ArrayBuffer.isView(item.value) || item.value instanceof ArrayBuffer)
          result += decoder.decode(item.value, { stream: true });
        else throw Object.assign(new TypeError('The "chunk" argument must be of type string or an instance of Buffer or TypedArray.'), { code: 'ERR_INVALID_ARG_TYPE' });
      }
      return result + decoder.decode();
    } catch (error) {
      try { if (reader) await reader.cancel(error); else await iterator.return?.(); } catch {}
      throw error;
    } finally { reader?.releaseLock(); }
  }
  async function json(stream) { return JSON.parse(await text(stream)); }
  return { arrayBuffer, blob, buffer, bytes, json, text };
}"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let global = context.global_root()?;
    let buffer_key = context.string_rooted("Buffer");
    let buffer = context.get_property_rooted(global, buffer_key)?;
    let decoder_key = context.string_rooted("TextDecoder");
    let decoder = context.get_property_rooted(global, decoder_key)?;
    let factory = context.evaluate_script_rooted(FACTORY, "node:stream/consumers/shared.js")?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[buffer, decoder])
}
