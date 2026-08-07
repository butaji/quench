const fs = require("fs");

(async () => {
  const path = `/tmp/quench-node-stage-178-${process.pid}`;
  fs.writeFileSync(path, "x");
  await new Promise((resolve, reject) =>
    fs.realpath(path, (error, result) => {
      if (error || result !== path) {
        reject(error || new Error("realpath result mismatch"));
      } else resolve();
    })
  );
  await new Promise((resolve) =>
    fs.realpath(path, null, (error, result) => {
      if (error || result !== path) {
        throw error || new Error("realpath null options mismatch");
      }
      resolve();
    })
  );
  await new Promise((resolve) =>
    fs.realpath("/tmp/quench-node-no-such-path", (error, result) => {
      if (!error || result !== undefined) {
        throw new Error("realpath error contract mismatch");
      }
      resolve();
    })
  );
})();
