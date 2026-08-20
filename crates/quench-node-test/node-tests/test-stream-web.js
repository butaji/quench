// Node compat: stream/web + text streams.
const web = globalThis;
const cons = require('node:stream/consumers');
if (typeof web.ReadableStream !== 'function') throw new Error('ReadableStream');
if (typeof web.WritableStream !== 'function') throw new Error('WritableStream');
if (typeof web.TransformStream !== 'function') throw new Error('TransformStream');
if (typeof web.TextDecoderStream !== 'function') throw new Error('TextDecoderStream');
const source = new web.ReadableStream({ start(c) { c.enqueue('a'); c.enqueue('b'); c.close(); } });
const reader = source.getReader();
reader.read().then(x => {
  if (x.value !== 'a' || x.done) throw new Error('stream read');
  return reader.read();
}).then(x => {
  if (x.value !== 'b' || x.done) throw new Error('stream read 2');
  console.log('stream/web+consumers: ok');
});
