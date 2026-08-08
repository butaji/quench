const { Readable, Writable, destroy } = require("stream");

const readable = new Readable({ read() {} });
destroy(readable);
readable.on("error", (error) => {
  if (error.name !== "AbortError") throw new Error("wrong readable error");
});
readable.on("close", () => {
  if (!readable.destroyed) throw new Error("readable was not destroyed");
});

const writable = new Writable({ write() {} });
destroy(writable);
writable.on("error", (error) => {
  if (error.name !== "AbortError") throw new Error("wrong writable error");
});
writable.on("close", () => {
  if (!writable.destroyed) throw new Error("writable was not destroyed");
});
