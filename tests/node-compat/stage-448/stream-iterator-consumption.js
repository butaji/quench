const { Readable } = require("stream");

(async () => {
  const stream = Readable.from(["a", "b"]);
  const data = [];
  stream.on("data", (chunk) => data.push(chunk));

  await new Promise((resolve) => queueMicrotask(resolve));
  const replay = [];
  for await (const chunk of stream) replay.push(chunk);

  if (data.join("") !== "ab") throw new Error("source delivery failed");
  if (replay.length !== 0) throw new Error("iterator replayed consumed chunks");

  console.log("stream iterator consumption passed");
})();
