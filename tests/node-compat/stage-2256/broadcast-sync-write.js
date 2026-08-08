const assert = require("assert");
const { broadcast, text } = require("stream/iter");

(async () => {
  const { writer, broadcast: bc } = broadcast({ budget: 4 });
  const consumer = bc.push();
  assert.strictEqual(writer.writeSync("test"), true);
  assert.strictEqual(writer.writeSync("x"), false);
  writer.endSync();
  assert.strictEqual(await text(consumer), "test");
  console.log("broadcast sync write passed");
})();
