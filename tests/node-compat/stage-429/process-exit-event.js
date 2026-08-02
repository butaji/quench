let called = false;
process.on("exit", (code) => {
  if (code !== 0) throw new Error("exit code was not zero");
  called = true;
});

process.on("exit", () => {
  if (!called) throw new Error("exit handlers were not ordered");
  console.log("process exit event passed");
});
