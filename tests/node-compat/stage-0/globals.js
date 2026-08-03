if (global !== globalThis) throw new Error("global alias");
if (process.argv[0] !== process.execPath) throw new Error("process.argv");
if (typeof process.nextTick !== "function") throw new Error("process.nextTick");
if (typeof Buffer !== "function") throw new Error("Buffer");
