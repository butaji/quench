// Node compat: stream/web + stream/consumers shape.
const web = require('node:stream/web');
const cons = require('node:stream/consumers');
if (typeof web.ReadableStream !== 'function') throw new Error('ReadableStream');
if (typeof cons.text !== 'function') throw new Error('text');
(async () => {
  const stream = new web.ReadableStream({
    start(controller) {
      controller.enqueue(new TextEncoder().encode('stream body'));
      controller.close();
    },
  });
  if (await cons.text(stream) !== 'stream body') throw new Error('consumers.text');
  console.log('stream/web+consumers: ok');
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
