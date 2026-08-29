const { inspect } = require('util');
const buffer = Buffer.from('12');

buffer.inspect = undefined;
buffer.extra = new Uint8Array(0);

const result = inspect(buffer);
if (result !== '<Buffer 31 32, inspect: undefined, extra: Uint8Array(0) []>') {
  throw new Error(`unexpected Buffer inspection: ${result}`);
}
