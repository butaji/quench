const timeout = setTimeout(() => {
  throw new Error("disposed timeout fired");
}, 100);
timeout[Symbol.dispose]();

const interval = setInterval(() => {
  throw new Error("disposed interval fired");
}, 100);
interval[Symbol.dispose]();

setTimeout(() => console.log("timer dispose passed"), 0);
