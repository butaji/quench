const fs = require("fs");
const path = require("path");

const source = path.resolve("tests/node/test/fixtures/copy/kitchen-sink");
const destination = path.join("/tmp", `quench-cp-${Date.now()}`);
fs.cp(source, destination, {
  recursive: true,
  dereference: true,
  filter: async (value) => {
    await new Promise((resolve) => setTimeout(resolve, 1));
    const stat = fs.statSync(value);
    return stat.isDirectory() || value.endsWith(".js");
  },
}, (error) => {
  if (error) {
    console.error(
      `cp diagnostic: ${
        String(error)
      } name=${error.name} code=${error.code} message=${error.message} keys=${
        Object.keys(error)
      }`,
    );
    throw error;
  }
  const verify = (directory) => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const child = path.join(directory, entry.name);
      if (entry.isDirectory()) verify(child);
      else if (!entry.name.endsWith(".js")) {
        throw new Error(`filter copied ${entry.name}`);
      }
    }
  };
  verify(destination);
  fs.rmSync(destination, { recursive: true, force: true });
});
