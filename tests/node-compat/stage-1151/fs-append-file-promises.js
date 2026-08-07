const fs = require("fs");

const filename = `append-promise-${Date.now()}.txt`;
fs.writeFileSync(filename, "before");
fs.promises
  .appendFile(filename, "after")
  .then(() => fs.promises.readFile(filename, "utf8"))
  .then((content) => {
    fs.unlinkSync(filename);
    if (content !== "beforeafter") {
      throw new Error(`unexpected content: ${content}`);
    }
  });
