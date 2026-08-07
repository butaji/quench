const util = require("util");
const result = util.format(new SharedArrayBuffer(4));
if (
  result !==
    "SharedArrayBuffer { [Uint8Contents]: <00 00 00 00>, [byteLength]: 4 }"
) {
  throw new Error(result);
}
