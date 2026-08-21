const assert = require("assert");
const vfs = require("node:vfs");
const { MemoryProvider } = require("internal/vfs/providers/memory");

(async () => {
  const provider = new MemoryProvider();
  const root = provider[
    Object.getOwnPropertySymbols(provider).find(
      (symbol) => symbol.description === "kRoot",
    )
  ];
  root.children.set("lazy", {
    type: 1,
    mode: 0o755,
    children: new Map(),
    populated: false,
    populate(scoped) {
      scoped.addFile("hello.txt", "hello");
    },
  });
  root.children.set("dynamic.txt", {
    type: 0,
    mode: 0o644,
    contentProvider: () => "dynamic",
  });
  root.children.set("async.txt", {
    type: 0,
    mode: 0o644,
    contentProvider: async () => "async",
  });

  const filesystem = vfs.create(provider);
  assert.deepStrictEqual(filesystem.readdirSync("/lazy"), ["hello.txt"]);
  assert.strictEqual(
    filesystem.readFileSync("/lazy/hello.txt", "utf8"),
    "hello",
  );
  assert.strictEqual(
    filesystem.readFileSync("/dynamic.txt", "utf8"),
    "dynamic",
  );
  assert.throws(() => filesystem.readFileSync("/async.txt"), {
    code: "ERR_INVALID_STATE",
  });
  assert.strictEqual(
    await filesystem.promises.readFile("/async.txt", "utf8"),
    "async",
  );
  console.log("dynamic memory provider passed");
})();
