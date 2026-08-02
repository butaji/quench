const { Writable } = require("stream");

const stream = new Writable({ highWaterMark: 4 });
let drains = 0;
stream.on("drain", () => drains++);

if (stream.write("a") !== true) throw new Error("small write rejected");
if (stream.writableLength !== 1) throw new Error("length was not tracked");
setTimeout(() => {
  if (drains !== 0) throw new Error("spurious drain event");
  if (stream.writableLength !== 0) throw new Error("length did not recover");
  console.log("stream drain transition passed");
}, 0);
