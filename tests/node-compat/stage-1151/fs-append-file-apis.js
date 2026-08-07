const fs = require("fs");

const callbackFile = `append-callback-${Date.now()}.txt`;
fs.writeFileSync(callbackFile, "before");
fs.appendFile(callbackFile, "after", (error) => {
  if (error) throw error;
  const content = fs.readFileSync(callbackFile, "utf8");
  fs.unlinkSync(callbackFile);
  if (content !== "beforeafter") {
    throw new Error(`unexpected callback content: ${content}`);
  }
});

const promiseFile = `append-promise-${Date.now()}.txt`;
fs.writeFileSync(promiseFile, "before");
fs.promises
  .appendFile(promiseFile, "after")
  .then(() => fs.promises.readFile(promiseFile, "utf8"))
  .then((content) => {
    fs.unlinkSync(promiseFile);
    if (content !== "beforeafter") {
      throw new Error(`unexpected promise content: ${content}`);
    }
  });
