const readline = require("readline");
const rl = readline.createInterface({ input: process.stdin, output: process.stdout, prompt: "> " });
rl.on("line", (line) => {
  const words = line.trim().split(/\s+/);
  console.log(words.length + " words: " + words.join(" ").toUpperCase());
  rl.prompt();
});
rl.on("close", () => console.log("done"));
rl.prompt();
